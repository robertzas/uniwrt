//! The support bundle (features/observability.md, Troubleshooting assistants; D-144): an
//! admin's `GET v1/support/bundle` answers one `.tar.gz` (`uniwrt-support-<UTC time>/…`) of
//! what the controller stores and what each online node shows now, with secrets left out or
//! masked, to attach to a bug report. Audited. What it holds is in `collect.rs`; the
//! controller's own log tail is [`LogTail`]'s.

mod collect;
mod tail;
#[cfg(test)]
mod tests;

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse as _, Response},
};
use uniwrt_core::Timestamp;
use uniwrt_model::user::{AuditAction, AuditEntry, Role};
use uniwrt_store::AuditRepo as _;

pub use tail::LogTail;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser};

/// `2026-11-24T04:02:33Z` as `20261124-0402`.
fn stamp(at: Timestamp) -> String {
    let text = at.to_string();
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    format!("{}-{}", &digits[..8], &digits[8..12])
}

pub(crate) async fn bundle(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
) -> ApiResult<Response> {
    if user.user.role != Role::Admin {
        return Err(ApiError::Forbidden(
            "Only admins download support bundles".to_owned(),
        ));
    }
    let now = api.now();
    let root = format!("uniwrt-support-{}", stamp(now));
    let files = collect::files(&api, now).await?;
    let paths: Vec<String> = files
        .iter()
        .map(|(path, _)| format!("{root}/{path}"))
        .collect();
    let entries: Vec<(&str, &[u8])> = paths
        .iter()
        .zip(&files)
        .map(|(path, (_, data))| (path.as_str(), data.as_slice()))
        .collect();
    let secs = u64::try_from(now.as_unix_ms() / 1_000).unwrap_or(0);
    let tarball = uniwrt_node::archive::pack(&entries, secs);
    let file = format!("{root}.tar.gz");
    api.store()
        .record(AuditEntry {
            at: now,
            action: AuditAction::SupportBundle,
            user_id: Some(user.user.id),
            username: Some(user.user.username.clone()),
            ip: Some(client.ip.to_string()),
            detail: Some(format!(
                "{file}, {} files, {} bytes",
                files.len(),
                tarball.len()
            )),
        })
        .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/gzip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        tarball,
    )
        .into_response())
}
