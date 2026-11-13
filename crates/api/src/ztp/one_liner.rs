//! The one-liner (features/adoption.md, Path D; D-103): a command to paste into any OpenWrt
//! shell that fetches the first-boot script with a single-use token, checks its SHA-256 and
//! runs it on the live device. The hello it installs carries the token, which pre-approves the
//! device.
//!
//! - With an admin's session: `POST /api/v1/ztp/one-liners` ([`ZtpOneLinerBody`]; 201 with
//!   the [`ZtpOneLiner`]), `GET` (its state, for the dialog to follow) and `DELETE` (withdraw)
//!   `/api/v1/ztp/one-liners/{token}`.
//! - Public: `GET /b/{token}` ([`bootstrap`], outside `/api` to keep the command short): the
//!   script, once.
//!
//! The script must be fetched within 15 minutes, and the hello must come within 15 minutes of
//! the fetch. One-liners live in memory with the script they serve (its hash is in the command),
//! at most [`MAX_HELD`], each for an hour: a restart withdraws them.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use data_encoding::BASE64URL_NOPAD;
use ring::rand::{SecureRandom as _, SystemRandom};
use serde::Deserialize;
use tracing::info;
use ts_rs::TS;
use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{
    node::NodeRole,
    ztp::{OneLinerState, ZtpOneLiner},
};
use uniwrt_node::ztp::{Endpoint, one_liner};

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

/// The one-liners held at once; a new one drops the oldest.
pub(crate) const MAX_HELD: usize = 16;
/// How long a one-liner's state stays readable.
const KEPT: Duration = Duration::from_hours(1);

/// Making a one-liner.
#[derive(Debug, Default, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ZtpOneLinerBody {
    /// An access point when left out; a gateway is refused (its WAN firewall would refuse the
    /// adoption's SSH).
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<NodeRole>,
    /// The display name; its hostname when left out.
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
}

pub(crate) async fn create(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<ZtpOneLinerBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ZtpOneLiner>)> {
    let user = user.user;
    super::admin(&user, "make one-liners")?;
    let Json(body) = body?;
    let role = body.role.unwrap_or(NodeRole::Ap);
    if role == NodeRole::Gateway {
        return Err(ApiError::BadRequest(
            "The one-liner sets up access points and bridges: a gateway's WAN firewall would \
             refuse the adoption"
                .to_owned(),
        ));
    }
    let name = body
        .name
        .map(|name| crate::adopt::display_name(&name))
        .transpose()?;
    let token = token()?;
    let (host, port) = super::script::split_host(client.host.as_deref());
    let seen_at = host
        .and_then(|host| host.parse::<IpAddr>().ok())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    let script = api
        .jobs()
        .ztp_script(role, port, seen_at, Some(&token))
        .await
        .map_err(jobs_error)?;
    let endpoint = command_endpoint(&script.endpoints, host, port);
    let (command, sha256) = one_liner(&endpoint, &token, &script.text);
    let now = api.now();
    let view = ZtpOneLiner {
        token: token.clone(),
        role,
        name,
        command,
        sha256,
        created_at: now,
        expires_at: now.saturating_add(ZtpOneLiner::LIFETIME),
        state: OneLinerState::Waiting,
        fetched_from: None,
        device: None,
        created_by: Some(user.username.clone()),
    };
    api.one_liners().add(view.clone(), script.text, now);
    super::audit(
        &api,
        &user,
        &client,
        format!(
            "made a one-liner for a{} {}",
            article(role),
            role_words(role)
        ),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

pub(crate) async fn get(
    State(api): State<Api>,
    user: CurrentUser,
    token: Result<Path<String>, PathRejection>,
) -> ApiResult<Json<ZtpOneLiner>> {
    super::admin(&user.user, "follow one-liners")?;
    let token = path_token(token)?;
    api.one_liners()
        .view(&token, api.now())
        .map(Json)
        .ok_or_else(not_found)
}

pub(crate) async fn withdraw(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    token: Result<Path<String>, PathRejection>,
) -> ApiResult<StatusCode> {
    let user = user.user;
    super::admin(&user, "withdraw one-liners")?;
    let token = path_token(token)?;
    if !api.one_liners().withdraw(&token, api.now()) {
        return Err(not_found());
    }
    super::audit(&api, &user, &client, "withdrew a one-liner".to_owned()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /b/{token}`, without a session.
pub fn bootstrap(api: Api) -> Router {
    Router::new()
        .route("/b/{token}", axum::routing::get(fetch))
        .with_state(api)
}

async fn fetch(State(api): State<Api>, client: Client, Path(token): Path<String>) -> Response {
    match api.one_liners().fetch(&token, client.ip, api.now()) {
        Some(script) => {
            info!(from = %client.ip, "a one-liner's script was fetched");
            (
                [
                    (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                script,
            )
                .into_response()
        }
        // Plain text: a shell reads it, and the command's hash check refuses it anyway.
        None => (
            StatusCode::NOT_FOUND,
            [(header::CACHE_CONTROL, "no-store")],
            "This one-liner was used or has expired: make a new one in uniwrt.\n",
        )
            .into_response(),
    }
}

/// Where the command fetches from: an IP address when the script knows one (the gateway's
/// record, else where the admin reached the controller), else the name the admin used, else
/// `uniwrt.<domain>`.
fn command_endpoint(endpoints: &[Endpoint], host: Option<&str>, port: u16) -> Endpoint {
    if let Some(ip) = endpoints
        .iter()
        .find(|endpoint| endpoint.host.parse::<IpAddr>().is_ok())
    {
        return ip.clone();
    }
    let named = host.filter(|host| {
        !host.is_empty()
            && host.len() <= 253
            && !host.starts_with(['-', '.'])
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            && *host != "localhost"
            // An IP address the endpoints left out is a loopback or unspecified one.
            && host.parse::<IpAddr>().is_err()
    });
    match (named, endpoints.first()) {
        (Some(host), _) => Endpoint {
            host: host.to_owned(),
            port,
        },
        (None, Some(first)) => first.clone(),
        (None, None) => Endpoint {
            host: "uniwrt.lan".to_owned(),
            port,
        },
    }
}

fn token() -> ApiResult<String> {
    let mut bytes = [0u8; 16];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ApiError::Internal("no randomness for a token".to_owned()))?;
    Ok(BASE64URL_NOPAD.encode(&bytes))
}

fn path_token(token: Result<Path<String>, PathRejection>) -> ApiResult<String> {
    token
        .map(|Path(token)| token)
        .map_err(|_| ApiError::BadRequest("A token is needed".to_owned()))
}

fn not_found() -> ApiError {
    ApiError::NotFound("No such one-liner: it was withdrawn, or the controller restarted".into())
}

fn article(role: NodeRole) -> &'static str {
    if role == NodeRole::Ap { "n" } else { "" }
}

fn role_words(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Gateway => "gateway",
        NodeRole::Ap => "access point",
        NodeRole::Bridge => "bridge",
    }
}

struct Held {
    view: ZtpOneLiner,
    script: String,
}

/// The one-liners, in memory.
#[derive(Default)]
pub(crate) struct OneLiners {
    held: Mutex<HashMap<String, Held>>,
}

impl OneLiners {
    fn lock(&self, now: Timestamp) -> MutexGuard<'_, HashMap<String, Held>> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|_, one| now < one.view.created_at.saturating_add(KEPT));
        for one in held.values_mut() {
            let open = matches!(
                one.view.state,
                OneLinerState::Waiting | OneLinerState::Fetched
            );
            if open && now >= one.view.expires_at {
                one.view.state = OneLinerState::Expired;
            }
        }
        held
    }

    pub(crate) fn add(&self, view: ZtpOneLiner, script: String, now: Timestamp) {
        let mut held = self.lock(now);
        while held.len() >= MAX_HELD {
            let oldest = held
                .iter()
                .min_by_key(|(_, one)| one.view.created_at)
                .map(|(token, _)| token.clone());
            match oldest {
                Some(token) => held.remove(&token),
                None => break,
            };
        }
        held.insert(view.token.clone(), Held { view, script });
    }

    pub(crate) fn view(&self, token: &str, now: Timestamp) -> Option<ZtpOneLiner> {
        self.lock(now).get(token).map(|one| one.view.clone())
    }

    /// The script, once and in time; from then on the hello has [`ZtpOneLiner::LIFETIME`].
    pub(crate) fn fetch(&self, token: &str, from: IpAddr, now: Timestamp) -> Option<String> {
        let mut held = self.lock(now);
        let one = held.get_mut(token)?;
        if one.view.state != OneLinerState::Waiting {
            return None;
        }
        one.view.state = OneLinerState::Fetched;
        one.view.fetched_from = Some(from);
        one.view.expires_at = now.saturating_add(ZtpOneLiner::LIFETIME);
        Some(one.script.clone())
    }

    /// Uses the token up for `device`'s hello, if it was fetched and is in time.
    pub(crate) fn redeem(
        &self,
        token: &str,
        device: MacAddr,
        now: Timestamp,
    ) -> Option<ZtpOneLiner> {
        let mut held = self.lock(now);
        let one = held.get_mut(token)?;
        if one.view.state != OneLinerState::Fetched {
            return None;
        }
        one.view.state = OneLinerState::Used;
        one.view.device = Some(device);
        Some(one.view.clone())
    }

    /// Undoes [`OneLiners::redeem`] when the adoption couldn't start: the next hello tries
    /// again.
    pub(crate) fn restore(&self, token: &str, now: Timestamp) {
        if let Some(one) = self.lock(now).get_mut(token)
            && one.view.state == OneLinerState::Used
        {
            one.view.state = OneLinerState::Fetched;
            one.view.device = None;
        }
    }

    pub(crate) fn withdraw(&self, token: &str, now: Timestamp) -> bool {
        let mut held = self.lock(now);
        let Some(one) = held.get_mut(token) else {
            return false;
        };
        if matches!(
            one.view.state,
            OneLinerState::Waiting | OneLinerState::Fetched
        ) {
            one.view.state = OneLinerState::Expired;
            one.view.expires_at = now;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::Timestamp;
    use uniwrt_model::{
        node::NodeRole,
        ztp::{OneLinerState, ZtpOneLiner},
    };

    use uniwrt_node::ztp::Endpoint;

    use super::{MAX_HELD, OneLiners, command_endpoint};

    const MIN: Duration = Duration::from_mins(1);

    fn at(minutes: u64) -> Timestamp {
        Timestamp::from_unix_secs(1_800_000_000)
            .saturating_add(MIN * u32::try_from(minutes).unwrap())
    }

    fn made(held: &OneLiners, token: &str, minute: u64) {
        let view = ZtpOneLiner {
            token: token.to_owned(),
            role: NodeRole::Ap,
            name: None,
            command: String::new(),
            sha256: String::new(),
            created_at: at(minute),
            expires_at: at(minute).saturating_add(ZtpOneLiner::LIFETIME),
            state: OneLinerState::Waiting,
            fetched_from: None,
            device: None,
            created_by: None,
        };
        held.add(view, format!("script {token}"), at(minute));
    }

    #[test]
    fn each_step_has_fifteen_minutes() {
        let held = OneLiners::default();
        let ip = "10.0.0.9".parse().unwrap();
        let mac = "94:83:c4:00:00:01".parse().unwrap();
        made(&held, "late", 0);
        assert_eq!(held.fetch("late", ip, at(15)), None, "fetched too late");
        assert_eq!(
            held.view("late", at(15)).unwrap().state,
            OneLinerState::Expired
        );

        made(&held, "slow", 0);
        assert_eq!(held.redeem("slow", mac, at(1)), None, "not fetched");
        assert_eq!(
            held.fetch("slow", ip, at(14)).as_deref(),
            Some("script slow")
        );
        assert_eq!(held.redeem("slow", mac, at(28)).unwrap().device, Some(mac));
        held.restore("slow", at(28));
        assert_eq!(
            held.view("slow", at(28)).unwrap().state,
            OneLinerState::Fetched
        );
        assert_eq!(
            held.redeem("slow", mac, at(28)).unwrap().state,
            OneLinerState::Used
        );
        assert_eq!(
            held.view("slow", at(59)).unwrap().state,
            OneLinerState::Used
        );
        assert_eq!(held.view("slow", at(60)), None, "kept for an hour");

        made(&held, "missed", 0);
        held.fetch("missed", ip, at(1));
        assert_eq!(
            held.redeem("missed", mac, at(16)),
            None,
            "the hello too late"
        );
    }

    #[test]
    fn a_new_one_drops_the_oldest_past_the_bound() {
        let held = OneLiners::default();
        for n in 0..=MAX_HELD {
            made(&held, &format!("t{n}"), n as u64);
        }
        assert_eq!(held.view("t0", at(20)), None);
        assert!(held.view("t1", at(20)).is_some());
        assert!(held.view(&format!("t{MAX_HELD}"), at(20)).is_some());
    }

    #[test]
    fn the_command_fetches_from_an_address_a_device_reaches() {
        let at = |host: &str| Endpoint {
            host: host.to_owned(),
            port: 8443,
        };
        let lan = at("uniwrt.lan");
        let both = [lan.clone(), at("10.0.0.5")];
        assert_eq!(
            command_endpoint(&both, Some("server.home"), 8443),
            at("10.0.0.5")
        );
        let named = [lan.clone()];
        assert_eq!(
            command_endpoint(&named, Some("server.home"), 8443),
            at("server.home")
        );
        for host in [
            Some("127.0.0.1"),
            Some("::1"),
            Some("localhost"),
            Some("a b"),
            None,
        ] {
            assert_eq!(command_endpoint(&named, host, 8443), lan, "{host:?}");
        }
    }
}
