//! `POST /api/v1/ztp/hello`, without a session (features/adoption.md, Path B; D-102): a ZTP
//! device's call home. The hello is checked ([`ZtpHello`]: a unicast MAC, an OpenSSH host key,
//! short printable strings), kept in the inbox under its MAC with the request's address (at
//! most [`MAX_PENDING`] devices, the least recently seen dropped), and answered with the
//! device's state. A hello with a one-liner's fetched token (Path D, D-103), else a
//! pre-approved MAC's first hello, uses it up and starts the adoption. Each client (IPv6: its /64) may send [`PER_WINDOW`] hellos per [`WINDOW`]; the
//! device's script sends one a minute.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use tokio::time::Instant;
use tracing::{info, warn};
use uniwrt_model::{
    job::JobState,
    user::{AuditAction, AuditEntry},
    ztp::{HelloAnswer, HelloState, PendingDevice, ZtpApproval, ZtpHello, ZtpOneLiner},
};
use uniwrt_node::HostKey;
use uniwrt_store::{AuditRepo as _, NodeRepo as _, ZtpRepo as _};

use crate::{Api, ApiError, ApiResult, Client};

/// The inbox's bound.
pub(crate) const MAX_PENDING: usize = 64;
pub(crate) const WINDOW: Duration = Duration::from_mins(10);
pub(crate) const PER_WINDOW: usize = 20;
const MAX_TRACKED: usize = 10_000;
const MAX_TEXT: usize = 128;

pub(crate) async fn hello(
    State(api): State<Api>,
    client: Client,
    body: Result<Json<ZtpHello>, JsonRejection>,
) -> ApiResult<Json<HelloAnswer>> {
    if let Some(wait) = api.hellos().hit(client.ip) {
        return Err(ApiError::TooManyRequests(wait));
    }
    let Json(hello) = body?;
    let device = checked(&hello, client.ip, &api)?;
    let mac = device.mac;
    let answer = |state| Ok(Json(HelloAnswer { state }));

    if api
        .store()
        .nodes()
        .await?
        .iter()
        .any(|node| node.mac == mac)
    {
        api.store().forget_ztp_pending(mac).await?;
        return answer(HelloState::Adopted);
    }
    let stored = api.store().ztp_hello(device, MAX_PENDING).await?;
    match super::job_state(&api, &stored).await? {
        Some(JobState::Running) => return answer(HelloState::Adopting),
        Some(JobState::Succeeded) => return answer(HelloState::Adopted),
        Some(JobState::Failed) | None => {}
    }
    let now = api.now();
    // A one-liner's token first, then a pre-approval of the MAC.
    let redeemed = hello
        .token
        .as_deref()
        .and_then(|token| api.one_liners().redeem(token, mac, now));
    let grant = if let Some(one_liner) = redeemed {
        Grant::OneLiner(one_liner)
    } else {
        let Some(approval) = api.store().take_ztp_approval(mac, now).await? else {
            info!(%mac, address = %stored.address, "a ZTP device waits for adoption");
            return answer(HelloState::Pending);
        };
        Grant::Approval(approval)
    };
    let (name, role, by, how) = match &grant {
        Grant::OneLiner(one) => (
            one.name.clone(),
            Some(one.role),
            &one.created_by,
            "a one-liner",
        ),
        Grant::Approval(approval) => (
            approval.name.clone(),
            approval.role,
            &approval.created_by,
            "pre-approved",
        ),
    };
    match super::start(&api, &stored, name, role, None).await {
        Ok(job) => {
            api.store()
                .record(AuditEntry {
                    at: api.now(),
                    action: AuditAction::Adopt,
                    user_id: None,
                    username: by.clone(),
                    ip: Some(client.ip.to_string()),
                    detail: Some(format!(
                        "{mac} {how} by {}, at {}, job {}",
                        by.as_deref().unwrap_or("an admin"),
                        stored.address,
                        job.id
                    )),
                })
                .await?;
            answer(HelloState::Adopting)
        }
        Err(err) => {
            // Not used after all: the next hello tries again.
            warn!(%mac, %err, "cannot adopt a {how} ZTP device");
            match grant {
                Grant::OneLiner(one) => api.one_liners().restore(&one.token, api.now()),
                Grant::Approval(approval) => api.store().put_ztp_approval(approval).await?,
            }
            answer(HelloState::Pending)
        }
    }
}

/// What lets a hello start its device's adoption.
enum Grant {
    OneLiner(ZtpOneLiner),
    Approval(ZtpApproval),
}

/// The hello as the inbox keeps it, or why it is refused.
fn checked(hello: &ZtpHello, address: IpAddr, api: &Api) -> ApiResult<PendingDevice> {
    if hello.mac.is_multicast() || hello.mac.octets() == [0; 6] {
        return Err(ApiError::BadRequest(format!(
            "{} is not a device's MAC address",
            hello.mac
        )));
    }
    let host_key = HostKey::parse(hello.host_key.trim()).map_err(|_| {
        ApiError::BadRequest("The host key must be an OpenSSH public key".to_owned())
    })?;
    let now = api.now();
    Ok(PendingDevice {
        mac: hello.mac,
        board_name: text("board_name", &hello.board_name, false)?,
        model: text("model", &hello.model, true)?,
        hostname: text("hostname", &hello.hostname, true)?,
        release: text("release", &hello.release, true)?,
        role: hello.role,
        address,
        host_key: host_key.to_openssh().to_owned(),
        fingerprint: host_key.fingerprint().to_owned(),
        first_seen: now,
        last_seen: now,
        job: None,
    })
}

/// A short printable field, trimmed; empty only where `may_be_empty`.
fn text(field: &str, value: &str, may_be_empty: bool) -> ApiResult<String> {
    let value = value.trim();
    let fits = value.chars().count() <= MAX_TEXT && !value.chars().any(char::is_control);
    if !fits || (value.is_empty() && !may_be_empty) {
        return Err(ApiError::BadRequest(format!(
            "{field} must be up to {MAX_TEXT} printable characters"
        )));
    }
    Ok(value.to_owned())
}

/// Hellos per client within [`WINDOW`], in memory and bounded; lost at restart, which only
/// lifts the limit early.
#[derive(Default)]
pub(crate) struct HelloLimiter {
    clients: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl HelloLimiter {
    /// Counts a hello from `ip`; how long it must wait instead, when over the limit.
    pub(crate) fn hit(&self, ip: IpAddr) -> Option<Duration> {
        let now = Instant::now();
        let mut clients = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        if clients.len() >= MAX_TRACKED && !clients.contains_key(&key(ip)) {
            clients.retain(|_, seen| {
                seen.back()
                    .is_some_and(|last| now.saturating_duration_since(*last) < WINDOW)
            });
            if clients.len() >= MAX_TRACKED {
                let oldest = clients
                    .iter()
                    .min_by_key(|(_, seen)| seen.back().copied())
                    .map(|(ip, _)| *ip);
                if let Some(oldest) = oldest {
                    clients.remove(&oldest);
                }
            }
        }
        let seen = clients.entry(key(ip)).or_default();
        while seen
            .front()
            .is_some_and(|first| now.saturating_duration_since(*first) >= WINDOW)
        {
            seen.pop_front();
        }
        if seen.len() >= PER_WINDOW {
            let first = *seen.front()?;
            return Some(WINDOW.saturating_sub(now.saturating_duration_since(first)));
        }
        seen.push_back(now);
        None
    }
}

/// IPv6 clients count per /64.
fn key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            IpAddr::from(octets)
        }
    }
}
