//! Zero-touch provisioning (features/adoption.md, Path B; D-022, D-102):
//!
//! - `POST /api/v1/ztp/hello` (public, rate-limited per client; [`hello`]): a device flashed
//!   with a ZTP image calls home; it lands in the pending inbox, or is adopted at once when its
//!   MAC was pre-approved. It answers [`HelloAnswer`].
//! - With an admin's session: `GET /api/v1/ztp/pending` ([`ZtpInbox`]), `DELETE
//!   /api/v1/ztp/pending/{mac}` (dismiss), `POST /api/v1/ztp/pending/{mac}/adopt`
//!   ([`ZtpAdoptBody`]; 202 with the job), `PUT` and `DELETE /api/v1/ztp/approvals/{mac}`
//!   ([`ZtpApprovalBody`]; single-use, 7 days), and `GET /api/v1/ztp/script?role=` (the
//!   first-boot script, [`script`]).
//!
//! Adopting from the inbox is Path A's job with the controller's key (the image trusts it), the
//! host key from the hello pinned, and the hello's board and MAC checked on the node.

pub(crate) mod hello;
pub(crate) mod script;
#[cfg(test)]
mod tests;

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{MacAddr, UserId};
use uniwrt_jobs::{AdoptRequest, ZtpClaim};
use uniwrt_model::{
    job::{Job, JobState},
    node::NodeRole,
    user::{AuditAction, AuditEntry, Role, User},
    ztp::{PendingDevice, ZtpApproval, ZtpInbox},
};
use uniwrt_node::{HostKey, Target};
use uniwrt_store::{AuditRepo as _, NodeRepo as _, ZtpRepo as _};

pub use uniwrt_model::ztp::HelloAnswer;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

/// The routes: the public hello (its body at most 4 KiB), and the admins' inbox, approvals and
/// script.
pub(crate) fn routes() -> axum::Router<Api> {
    use axum::routing::{delete, get, post, put};
    axum::Router::new()
        .route(
            "/v1/ztp/hello",
            post(hello::hello).layer(axum::extract::DefaultBodyLimit::max(4096)),
        )
        .route("/v1/ztp/pending", get(inbox))
        .route("/v1/ztp/pending/{mac}", delete(dismiss))
        .route("/v1/ztp/pending/{mac}/adopt", post(adopt))
        .route("/v1/ztp/approvals/{mac}", put(approve).delete(withdraw))
        .route("/v1/ztp/script", get(script::script))
}

/// Adopting a device from the inbox.
#[derive(Debug, Default, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ZtpAdoptBody {
    /// The display name; its hostname when left out.
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    /// The role its image was built for, else inferred, when left out.
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<NodeRole>,
}

/// Pre-approving a MAC: its first hello adopts it with these.
#[derive(Debug, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ZtpApprovalBody {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<NodeRole>,
}

pub(crate) async fn inbox(State(api): State<Api>, user: CurrentUser) -> ApiResult<Json<ZtpInbox>> {
    admin(&user.user, "see the devices waiting for adoption")?;
    let adopted: Vec<MacAddr> = api
        .store()
        .nodes()
        .await?
        .into_iter()
        .map(|node| node.mac)
        .collect();
    let mut pending = Vec::new();
    for device in api.store().ztp_pending().await? {
        if is_adopted(&api, &device, &adopted).await? {
            api.store().forget_ztp_pending(device.mac).await?;
        } else {
            pending.push(device);
        }
    }
    let approvals = api.store().ztp_approvals(api.now()).await?;
    Ok(Json(ZtpInbox { pending, approvals }))
}

pub(crate) async fn dismiss(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Result<Path<String>, PathRejection>,
) -> ApiResult<StatusCode> {
    let user = user.user;
    admin(&user, "dismiss devices")?;
    let mac = path_mac(mac)?;
    if !api.store().forget_ztp_pending(mac).await? {
        return Err(not_pending(mac));
    }
    audit(&api, &user, &client, format!("dismissed {mac}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn adopt(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Result<Path<String>, PathRejection>,
    body: Result<Json<ZtpAdoptBody>, JsonRejection>,
) -> ApiResult<Response> {
    let user = user.user;
    admin(&user, "adopt devices")?;
    let mac = path_mac(mac)?;
    let Json(body) = body?;
    let name = body
        .name
        .map(|name| crate::adopt::display_name(&name))
        .transpose()?;
    let device = api
        .store()
        .ztp_pending_device(mac)
        .await?
        .ok_or_else(|| not_pending(mac))?;
    let job = start(&api, &device, name, body.role, Some(user.id)).await?;
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Adopt,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "{mac} from the inbox at {}, job {}",
                device.address, job.id
            )),
        })
        .await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

pub(crate) async fn approve(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Result<Path<String>, PathRejection>,
    body: Result<Json<ZtpApprovalBody>, JsonRejection>,
) -> ApiResult<Json<ZtpApproval>> {
    let user = user.user;
    admin(&user, "pre-approve devices")?;
    let mac = path_mac(mac)?;
    let Json(body) = body?;
    if mac.is_multicast() || mac.octets() == [0; 6] {
        return Err(ApiError::BadRequest(format!(
            "{mac} is not a device's MAC address"
        )));
    }
    if let Some(node) = api
        .store()
        .nodes()
        .await?
        .into_iter()
        .find(|node| node.mac == mac)
    {
        return Err(ApiError::Conflict(format!(
            "{mac} is adopted already, as {}",
            node.name
        )));
    }
    let now = api.now();
    let approval = ZtpApproval {
        mac,
        name: body
            .name
            .map(|name| crate::adopt::display_name(&name))
            .transpose()?,
        role: body.role,
        created_at: now,
        expires_at: now.saturating_add(ZtpApproval::LIFETIME),
        created_by: Some(user.username.clone()),
    };
    api.store().put_ztp_approval(approval.clone()).await?;
    audit(
        &api,
        &user,
        &client,
        format!("pre-approved {mac} for 7 days"),
    )
    .await?;
    Ok(Json(approval))
}

pub(crate) async fn withdraw(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    mac: Result<Path<String>, PathRejection>,
) -> ApiResult<StatusCode> {
    let user = user.user;
    admin(&user, "withdraw pre-approvals")?;
    let mac = path_mac(mac)?;
    if !api.store().delete_ztp_approval(mac).await? {
        return Err(ApiError::NotFound(format!("{mac} is not pre-approved")));
    }
    audit(
        &api,
        &user,
        &client,
        format!("withdrew the pre-approval of {mac}"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Starts adopting a device from the inbox (by an admin, or by its pre-approval when `by` is
/// `None`), and notes the job on its row.
pub(crate) async fn start(
    api: &Api,
    device: &PendingDevice,
    name: Option<String>,
    role: Option<NodeRole>,
    by: Option<UserId>,
) -> ApiResult<Job> {
    let host_key = HostKey::parse(&device.host_key)
        .map_err(|_| ApiError::Internal("a stored hello holds no host key".to_owned()))?;
    let request = AdoptRequest {
        target: Target::new(device.address.to_string(), 22),
        host_key,
        // The image trusts the controller's key.
        password: None,
        remember_password: false,
        name,
        role: role.or(device.role),
        ztp: Some(ZtpClaim {
            mac: device.mac,
            board_name: device.board_name.clone(),
        }),
    };
    let job = api.jobs().adopt(request, by).await.map_err(jobs_error)?;
    api.store().set_ztp_job(device.mac, job.id).await?;
    Ok(job)
}

/// Whether the device is adopted: a node has its MAC, or its adoption job succeeded (a node
/// whose identity is another of its MACs).
pub(crate) async fn is_adopted(
    api: &Api,
    device: &PendingDevice,
    adopted: &[MacAddr],
) -> ApiResult<bool> {
    if adopted.contains(&device.mac) {
        return Ok(true);
    }
    Ok(job_state(api, device).await? == Some(JobState::Succeeded))
}

/// The state of the device's latest adoption job, if it is still in the history.
pub(crate) async fn job_state(api: &Api, device: &PendingDevice) -> ApiResult<Option<JobState>> {
    let Some(id) = device.job else {
        return Ok(None);
    };
    Ok(api
        .jobs()
        .job(id)
        .await
        .map_err(jobs_error)?
        .map(|job| job.state))
}

fn admin(user: &User, what: &str) -> ApiResult<()> {
    if user.role == Role::Admin {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("Only admins {what}")))
    }
}

fn path_mac(mac: Result<Path<String>, PathRejection>) -> ApiResult<MacAddr> {
    let Path(mac) = mac.map_err(|_| ApiError::BadRequest("A MAC address is needed".to_owned()))?;
    mac.parse()
        .map_err(|_| ApiError::BadRequest(format!("{mac} is not a MAC address")))
}

fn not_pending(mac: MacAddr) -> ApiError {
    ApiError::NotFound(format!("{mac} is not waiting for adoption"))
}

async fn audit(api: &Api, user: &User, client: &Client, detail: String) -> ApiResult<()> {
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Ztp,
            user_id: Some(user.id),
            username: Some(user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(detail),
        })
        .await?;
    Ok(())
}
