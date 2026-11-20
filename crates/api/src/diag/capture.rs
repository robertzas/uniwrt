//! A packet capture's download (features/observability.md, Diagnostics toolkit; D-141, D-142):
//! an admin's `GET v1/nodes/{id}/diag/capture` ([`CaptureQuery`] as its query string) runs
//! tcpdump on any adopted node, observe mode included (it changes nothing), and answers the
//! `.pcap` as it is captured, a streamed attachment that ends at the query's time or size cap,
//! when tcpdump ends, or when the download is cancelled (dropping the body ends tcpdump). The
//! capture is audited once it runs. 400 for a query (or a device or filter) tcpdump refuses, 409
//! while the node is offline, lacks tcpdump or captures already, 502 when it can't start.
//! A `GET`, so that the browser downloads it to disk with its own progress and cancel: the
//! session cookie is `SameSite=Strict`, so another site can't start one.

use std::convert::Infallible;

use axum::{
    body::Body,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::header,
    response::{IntoResponse as _, Response},
    routing::get,
};
use uniwrt_core::Timestamp;
use uniwrt_engine::CaptureError;
use uniwrt_model::{
    diag::capture::CaptureQuery,
    node::Node,
    user::{AuditAction, AuditEntry},
};
use uniwrt_store::AuditRepo as _;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

pub(super) fn routes() -> axum::Router<Api> {
    axum::Router::new().route("/v1/nodes/{id}/diag/capture", get(capture))
}

async fn capture(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<CaptureQuery>, QueryRejection>,
) -> ApiResult<Response> {
    let user = super::admin(user, "Only admins capture packets")?;
    let node = super::tables::node(&api, id).await?;
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let capture = api
        .diag()
        .capture(node.id, &query)
        .await
        .map_err(|err| refused(&node, &err))?;
    let filter = query
        .filter()
        .map_or_else(String::new, |filter| format!(" ({filter})"));
    api.store()
        .record(AuditEntry {
            at: api.now(),
            action: AuditAction::Diagnostics,
            user_id: Some(user.id),
            username: Some(user.username),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "capture on {} of {}{filter}, up to {} s and {} MiB",
                query.interface, node.name, query.seconds, query.max_mib
            )),
        })
        .await?;
    let file = file_name(&node.name, &query.interface, api.now());
    let chunks = futures_util::stream::unfold(Some((capture, node.name)), |state| async move {
        let (mut capture, name) = state?;
        if let Some(bytes) = capture.next().await {
            return Some((Ok::<_, Infallible>(bytes), Some((capture, name))));
        }
        tracing::info!(
            node = %name,
            bytes = capture.bytes(),
            packets = capture.packets(),
            end = ?capture.end(),
            "a packet capture ended"
        );
        None
    });
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.tcpdump.pcap".to_owned(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        Body::from_stream(chunks),
    )
        .into_response())
}

fn refused(node: &Node, err: &CaptureError) -> ApiError {
    match err {
        CaptureError::Invalid(_) | CaptureError::Refused(_) => {
            ApiError::BadRequest(err.to_string())
        }
        CaptureError::Offline => ApiError::Conflict(format!("{} is offline", node.name)),
        CaptureError::Busy(_) | CaptureError::Missing(_) => ApiError::Conflict(err.to_string()),
        CaptureError::Failed(_) => ApiError::BadGateway(err.to_string()),
    }
}

/// `<node>-<device>-<UTC date>-<time>.pcap`, of characters safe in any file system.
fn file_name(node: &str, device: &str, at: Timestamp) -> String {
    let safe = |text: &str| -> String {
        text.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let digits: String = at
        .to_string()
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect();
    let (date, time) = digits.split_at(digits.len().min(8));
    format!("{}-{}-{date}-{time}.pcap", safe(node), safe(device))
}

#[cfg(test)]
mod tests;
