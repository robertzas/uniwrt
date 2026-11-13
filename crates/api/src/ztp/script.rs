//! `GET /api/v1/ztp/script?role=ap|bridge|gateway` (admins; D-102): the first-boot script of a
//! zero-touch image, for the OpenWrt Firmware Selector's "script to run on first boot" or an
//! image builder's `/etc/uci-defaults`. It calls home on the HTTPS port the admin used, to
//! `uniwrt.<domain>` and then the controller's address: the gateway's record of it, else the
//! IP address the admin used.

use std::net::IpAddr;

use axum::{
    extract::{Query, State, rejection::QueryRejection},
    http::header,
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use uniwrt_model::node::NodeRole;

use crate::{Api, ApiError, ApiResult, Client, CurrentUser, jobs::jobs_error};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScriptQuery {
    /// An access point when left out.
    #[serde(default)]
    role: Option<NodeRole>,
}

pub(crate) async fn script(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    query: Result<Query<ScriptQuery>, QueryRejection>,
) -> ApiResult<Response> {
    super::admin(&user.user, "download the first-boot script")?;
    let Query(query) = query.map_err(|err| ApiError::BadRequest(err.body_text()))?;
    let script = first_boot(&api, &client, query.role.unwrap_or(NodeRole::Ap)).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"99-uniwrt\"",
            ),
        ],
        script,
    )
        .into_response())
}

/// The first-boot script of an image of `role`, calling home where the admin's `client`
/// reached the controller.
pub(crate) async fn first_boot(api: &Api, client: &Client, role: NodeRole) -> ApiResult<String> {
    let (host, port) = split_host(client.host.as_deref());
    let seen_at = host
        .and_then(|host| host.parse::<IpAddr>().ok())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    Ok(api
        .jobs()
        .ztp_script(role, port, seen_at, None)
        .await
        .map_err(jobs_error)?
        .text)
}

/// The host and port of a `Host` value (`name`, `name:port`, `[v6]:port`); 443 without one.
pub(crate) fn split_host(host: Option<&str>) -> (Option<&str>, u16) {
    let Some(host) = host else {
        return (None, 443);
    };
    if let Some(rest) = host.strip_prefix('[') {
        let Some((v6, tail)) = rest.split_once(']') else {
            return (None, 443);
        };
        let port = tail.strip_prefix(':').and_then(|port| port.parse().ok());
        return (Some(v6), port.unwrap_or(443));
    }
    match host.rsplit_once(':') {
        Some((name, port)) => (Some(name), port.parse().unwrap_or(443)),
        None => (Some(host), 443),
    }
}

#[cfg(test)]
mod tests {
    use super::split_host;

    #[test]
    fn hosts_and_ports() {
        assert_eq!(split_host(None), (None, 443));
        assert_eq!(split_host(Some("uniwrt.lan")), (Some("uniwrt.lan"), 443));
        assert_eq!(split_host(Some("10.0.0.5:8443")), (Some("10.0.0.5"), 8443));
        assert_eq!(split_host(Some("[fd00::5]:8443")), (Some("fd00::5"), 8443));
        assert_eq!(split_host(Some("[fd00::5]")), (Some("fd00::5"), 443));
    }
}
