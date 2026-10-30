//! `POST /api/v1/adopt/batch`: adopts several nodes with one password, typically those that
//! `GET /api/v1/discovered` reports, each with the host key the user confirmed. Every node is
//! checked before any job starts; each then gets its own job, as from `POST /api/v1/adopt`.
//! Answers 202 with the jobs started, and the nodes refused because a job runs for them already.

use std::collections::BTreeSet;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_jobs::AdoptRequest;
use uniwrt_model::{job::Job, node::NodeRole};

use super::{SSH_PORT, audit, display_name, host_key, password, remember, target};
use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

/// The most nodes in one batch.
const MAX_NODES: usize = 64;

/// Adopting several nodes with one password. Not `Debug`: it holds a password.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct BatchAdoptBody {
    /// 1 to 64 nodes, each at its own address.
    pub nodes: Vec<BatchNode>,
    /// The root password of every node; left out when they trust the controller's key already.
    #[serde(default)]
    #[ts(optional)]
    pub password: Option<String>,
    /// Keep the password in the vault, for each node.
    #[serde(default)]
    #[ts(optional)]
    pub remember_password: Option<bool>,
}

/// One node of a batch, as in `POST /api/v1/adopt`.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct BatchNode {
    pub host: String,
    /// 22 when left out.
    #[serde(default)]
    #[ts(optional)]
    pub port: Option<u16>,
    /// The host key the user confirmed.
    pub host_key: String,
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub role: Option<NodeRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BatchAdoptResult {
    /// In the order of the request.
    pub jobs: Vec<Job>,
    pub refused: Vec<Refused>,
}

/// A node not adopted, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Refused {
    pub host: String,
    pub port: u16,
    pub reason: String,
}

pub(crate) async fn adopt(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<BatchAdoptBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<BatchAdoptResult>)> {
    let Json(body) = body?;
    if !(1..=MAX_NODES).contains(&body.nodes.len()) {
        return Err(ApiError::BadRequest(format!(
            "A batch has 1 to {MAX_NODES} nodes"
        )));
    }
    let password = password(body.password)?;
    let remember_password = remember(body.remember_password, password.as_ref())?;
    let mut targets = BTreeSet::new();
    let mut requests = Vec::with_capacity(body.nodes.len());
    for (i, node) in body.nodes.into_iter().enumerate() {
        let numbered = |err| match err {
            ApiError::BadRequest(detail) => {
                ApiError::BadRequest(format!("Node {}: {detail}", i + 1))
            }
            err => err,
        };
        let target = target(&node.host, node.port.unwrap_or(SSH_PORT)).map_err(numbered)?;
        if !targets.insert(target.to_string().to_lowercase()) {
            return Err(numbered(ApiError::BadRequest(format!(
                "{}:{} is in the batch twice",
                target.host, target.port
            ))));
        }
        requests.push(AdoptRequest {
            host_key: host_key(&node.host_key).map_err(numbered)?,
            password: password.clone(),
            remember_password,
            name: node
                .name
                .map(|name| display_name(&name))
                .transpose()
                .map_err(numbered)?,
            role: node.role,
            target,
        });
    }
    if !api.jobs().can_adopt() {
        return Err(jobs_error(uniwrt_jobs::Error::Unavailable));
    }
    let user = user.user;
    let mut result = BatchAdoptResult {
        jobs: Vec::with_capacity(requests.len()),
        refused: Vec::new(),
    };
    for request in requests {
        let (host, port) = (request.target.host.clone(), request.target.port);
        match api.jobs().adopt(request, Some(user.id)).await {
            Ok(job) => {
                audit(&api, &user, &client, &job).await?;
                result.jobs.push(job);
            }
            Err(uniwrt_jobs::Error::Busy { .. }) => result.refused.push(Refused {
                host,
                port,
                reason: "A job is running for it already".to_owned(),
            }),
            Err(err) => return Err(jobs_error(err)),
        }
    }
    Ok((StatusCode::ACCEPTED, Json(result)))
}
