//! Replacing a node's device and resetting a node, with an admin's session (features/
//! lifecycle.md, Replace / RMA / decommission; D-131), each a job answered 202 with it:
//!
//! - `POST v1/nodes/{id}/replace` with the new device's address, its confirmed host key and the
//!   root password, as an adoption takes them (`POST v1/adopt/probe` reads the key): the device
//!   is adopted into the node's record (its id, name, role and history stay), and a revision of
//!   kind `replace` moves the node's settings to it; the job's Record step lists what had no
//!   match on it. The node is in observe mode until management is enabled and applied.
//! - `POST v1/nodes/{id}/reset`: `firstboot` and a restart on the node; its record then awaits
//!   the device, with a pre-approval of its MAC and board (`v1/ztp/pending` lists it, its
//!   `node` set), so whichever path finds the device again adopts it back into the record.
//!
//! Refused (409) for a node an active rollout still has to upgrade and while an apply or
//! another job for the node runs; a reset also for the gateway while other nodes are adopted.
//! Audited (`replace`).

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    response::Response,
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_jobs::AdoptRequest;
use uniwrt_model::user::AuditAction;

use crate::{
    Api, ApiResult, Client, CurrentUser,
    adopt::{SSH_PORT, host_key, password, remember, target},
    decommission::{admin, audited, author, node_id},
    jobs::jobs_error,
};

/// The new device of a node. Not `Debug`: it holds a password.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ReplaceBody {
    pub host: String,
    /// 22 when left out.
    #[serde(default)]
    #[ts(optional)]
    pub port: Option<u16>,
    /// The host key the user confirmed, as the probe returned it.
    pub host_key: String,
    /// The root password; left out when the device trusts the controller's key already, empty
    /// for one without (factory defaults).
    #[serde(default)]
    #[ts(optional)]
    pub password: Option<String>,
    /// Keep the password in the vault, for emergencies.
    #[serde(default)]
    #[ts(optional)]
    pub remember_password: Option<bool>,
}

pub(crate) async fn replace(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<ReplaceBody>, JsonRejection>,
) -> ApiResult<Response> {
    let id = node_id(id)?;
    let user = admin(user)?;
    let Json(body) = body?;
    let password = password(body.password)?;
    let remember_password = remember(body.remember_password, password.as_ref())?;
    let request = AdoptRequest {
        target: target(&body.host, body.port.unwrap_or(SSH_PORT))?,
        host_key: host_key(&body.host_key)?,
        password,
        remember_password,
        name: None,
        role: None,
        ztp: None,
        port: None,
        replace: None,
    };
    let job = api
        .jobs()
        .replace(id, request, Some(author(&user)))
        .await
        .map_err(jobs_error)?;
    let how = format!("replaced with the device at {} the device of", body.host);
    audited(&api, &user, &client, job, AuditAction::Replace, &how).await
}

pub(crate) async fn reset(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
) -> ApiResult<Response> {
    let id = node_id(id)?;
    let user = admin(user)?;
    let job = api
        .jobs()
        .reset(id, Some(author(&user)))
        .await
        .map_err(jobs_error)?;
    audited(&api, &user, &client, job, AuditAction::Replace, "reset").await
}

#[cfg(test)]
#[path = "replace_tests.rs"]
mod tests;
