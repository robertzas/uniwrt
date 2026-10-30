//! Adoption, with a session (features/adoption.md, Path A): `POST /api/v1/adopt/probe` reads a
//! node's host key for the user to confirm, and `POST /api/v1/adopt` starts adopting the node
//! with that key, answering 202 with the job, which `GET /api/v1/jobs/{id}` and the WS hub's
//! `jobs` topic follow. `POST /api/v1/adopt/batch` adopts several with one password
//! ([`batch`]).

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_jobs::AdoptRequest;
use uniwrt_model::{
    job::Job,
    node::NodeRole,
    user::{AuditAction, AuditEntry, User},
};
use uniwrt_node::{HostKey, Target};
use uniwrt_store::AuditRepo as _;
use zeroize::Zeroizing;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

/// The longest host name (RFC 1035) or address.
const MAX_HOST: usize = 253;
const MAX_NAME_CHARS: usize = 64;
/// As for controller users' passwords.
const MAX_PASSWORD_BYTES: usize = 1024;

/// The port when a request leaves it out.
const SSH_PORT: u16 = 22;

/// Where a node's SSH server listens.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ProbeRequest {
    /// An IP address or a host name.
    pub host: String,
    /// 22 when left out.
    #[serde(default)]
    #[ts(optional)]
    pub port: Option<u16>,
}

/// The host key a node presented, for the user to compare with the node's own
/// (`dropbearkey -y -f /etc/dropbear/dropbear_ed25519_host_key` on it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ProbeResult {
    pub host: String,
    pub port: u16,
    /// OpenSSH form, which `POST /api/v1/adopt` takes back once the user confirmed it.
    pub host_key: String,
    /// SHA-256, as OpenSSH shows it: `SHA256:…`.
    pub fingerprint: String,
    pub algorithm: String,
}

/// Adopting a node. Not `Debug`: it holds a password.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AdoptBody {
    pub host: String,
    /// 22 when left out.
    #[serde(default)]
    #[ts(optional)]
    pub port: Option<u16>,
    /// The host key the user confirmed, as the probe returned it.
    pub host_key: String,
    /// The root password, to install the controller's key; left out when the node trusts the
    /// key already. Empty for a node without one (factory defaults).
    #[serde(default)]
    #[ts(optional)]
    pub password: Option<String>,
    /// Keep the password in the vault, for emergencies.
    #[serde(default)]
    #[ts(optional)]
    pub remember_password: Option<bool>,
    /// The display name; the node's hostname when left out.
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// Inferred from the node's interfaces when left out.
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<NodeRole>,
}

pub(crate) async fn probe(
    State(api): State<Api>,
    _: CurrentUser,
    body: Result<Json<ProbeRequest>, JsonRejection>,
) -> ApiResult<Json<ProbeResult>> {
    let Json(ProbeRequest { host, port }) = body?;
    let target = target(&host, port.unwrap_or(SSH_PORT))?;
    let host_key = api.jobs().probe(&target).await.map_err(jobs_error)?;
    Ok(Json(ProbeResult {
        host: target.host,
        port: target.port,
        host_key: host_key.to_openssh().to_owned(),
        fingerprint: host_key.fingerprint().to_owned(),
        algorithm: host_key.algorithm(),
    }))
}

pub(crate) async fn adopt(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<AdoptBody>, JsonRejection>,
) -> ApiResult<Response> {
    let Json(body) = body?;
    let password = password(body.password)?;
    let remember_password = remember(body.remember_password, password.as_ref())?;
    let request = AdoptRequest {
        target: target(&body.host, body.port.unwrap_or(SSH_PORT))?,
        host_key: host_key(&body.host_key)?,
        password,
        remember_password,
        name: body.name.map(|name| display_name(&name)).transpose()?,
        role: body.role,
    };
    let user = user.user;
    let job = api
        .jobs()
        .adopt(request, Some(user.id))
        .await
        .map_err(jobs_error)?;
    audit(&api, &user, &client, &job).await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

/// Records who started adopting a node, and from where.
async fn audit(api: &Api, user: &User, client: &Client, job: &Job) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Adopt,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!("{}, job {}", job.target, job.id)),
        })
        .await?;
    Ok(())
}

fn password(password: Option<String>) -> ApiResult<Option<Zeroizing<String>>> {
    let password = password.map(Zeroizing::new);
    if password
        .as_ref()
        .is_some_and(|password| password.len() > MAX_PASSWORD_BYTES)
    {
        return Err(ApiError::BadRequest(format!(
            "A password is at most {MAX_PASSWORD_BYTES} bytes long"
        )));
    }
    Ok(password)
}

fn remember(remember: Option<bool>, password: Option<&Zeroizing<String>>) -> ApiResult<bool> {
    let remember = remember.unwrap_or(false);
    if remember && password.is_none() {
        return Err(ApiError::BadRequest(
            "There is no password to remember".to_owned(),
        ));
    }
    Ok(remember)
}

fn host_key(host_key: &str) -> ApiResult<HostKey> {
    HostKey::parse(host_key.trim())
        .map_err(|_| ApiError::BadRequest("The host key must be an OpenSSH public key".to_owned()))
}

/// `root` at `host:port`: a host name or an IP address (IPv6 without brackets), never anything
/// a shell or SSH option could read otherwise.
fn target(host: &str, port: u16) -> ApiResult<Target> {
    let host = host.trim();
    let valid = !host.is_empty()
        && host.len() <= MAX_HOST
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_' | '%'));
    if !valid {
        return Err(ApiError::BadRequest(
            "The host must be an IP address or a host name".to_owned(),
        ));
    }
    if port == 0 {
        return Err(ApiError::BadRequest(
            "The port must be 1 to 65535".to_owned(),
        ));
    }
    Ok(Target::new(host, port))
}

fn display_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ApiError::BadRequest(format!(
            "A name is 1 to {MAX_NAME_CHARS} characters, without control characters"
        )));
    }
    Ok(name.to_owned())
}

pub(crate) mod batch;
#[cfg(test)]
mod tests;
