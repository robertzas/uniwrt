//! The WAN wizard's API (features/wan.md; D-075):
//!
//! - `GET /api/v1/isp/presets`: every ISP preset ([`Preset`]), generic ones first, then by
//!   country and name.
//! - `POST /api/v1/wan/test` (admins; audited) with a [`WanTestBody`]: tries the WAN on the
//!   gateway and rolls it back, answering 202 with the job (`wan_test`), which
//!   `GET /api/v1/jobs/{id}` and the WS hub's `jobs` topic follow. The WAN must be valid in
//!   the latest revision (else 422 with its issues); the gateway must be managed (409).
//!   Nothing is saved: the wizard saves the revision after.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;
use ts_rs::TS;
use uniwrt_isp::Preset;
use uniwrt_model::{
    site::{
        SecretRef, check_secret,
        gateway::{WanConfig, WanIpv6, WanProto},
        validate::{Issue, Severity},
    },
    user::AuditAction,
};
use uniwrt_store::RevisionRepo as _;
use zeroize::Zeroizing;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    jobs::jobs_error,
    site::{checked, ensure_first_revision},
};

/// A WAN to try.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct WanTestBody {
    pub wan: WanConfig,
    /// A new PPPoE password, not saved; without it, the one `wan` refers to.
    #[serde(default)]
    #[ts(optional)]
    pub password: Option<String>,
    /// The IPv6 to try when DHCPv6 delegates no prefix: the preset's 6rd for `auto`.
    #[serde(default)]
    #[ts(optional)]
    pub fallback: Option<WanIpv6>,
}

pub(crate) async fn presets(_user: CurrentUser) -> Json<&'static [Preset]> {
    Json(uniwrt_isp::presets())
}

pub(crate) async fn test(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<WanTestBody>, JsonRejection>,
) -> ApiResult<Response> {
    admin(&user, "test the WAN")?;
    let Json(WanTestBody {
        wan,
        password,
        fallback,
    }) = body?;
    ensure_first_revision(&api, None).await?;
    let head = api
        .store()
        .head_revision()
        .await?
        .ok_or_else(|| ApiError::Conflict("The site has no revision yet".to_owned()))?;
    let password = password.map(Zeroizing::new);
    if let Some(value) = &password
        && let Err(why) = check_secret("wan/password", value)
    {
        let issue = Issue {
            path: "wan/password".to_owned(),
            severity: Severity::Error,
            message: why.to_owned(),
        };
        return Err(ApiError::Invalid(why.to_owned(), vec![issue]));
    }
    // Validated as the site would hold it; a new password stands in for the one it refers to.
    let mut site = head.config;
    site.wan = wan.clone();
    if password.is_some()
        && let WanProto::Pppoe { password: slot, .. } = &mut site.wan.proto
    {
        *slot = SecretRef::new(format!("config/{}", "0".repeat(32)));
    }
    let mut trials = vec![site.clone()];
    if let Some(fallback) = &fallback {
        let mut with = site;
        with.wan.ipv6 = fallback.clone();
        trials.push(with);
    }
    for trial in &trials {
        checked(&api, trial).await.map_err(wan_only)?;
    }
    let test = uniwrt_jobs::WanTest {
        wan,
        password,
        fallback,
    };
    let job = api
        .jobs()
        .test_wan(test, Some(user.user.id))
        .await
        .map_err(test_error)?;
    audit(
        &api,
        &user,
        &client,
        AuditAction::WanTest,
        format!("job {}", job.id),
    )
    .await?;
    let location = format!("/api/v1/jobs/{}", job.id);
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(job),
    )
        .into_response())
}

/// A 422 for the WAN's own issues; the rest of the site was valid when saved.
fn wan_only(err: ApiError) -> ApiError {
    match err {
        ApiError::Invalid(detail, issues) => {
            let wan: Vec<Issue> = issues
                .into_iter()
                .filter(|i| i.path.starts_with("wan"))
                .collect();
            if wan.is_empty() {
                ApiError::Conflict(format!("The latest revision is not valid: {detail}"))
            } else {
                ApiError::Invalid("The WAN has errors".to_owned(), wan)
            }
        }
        err => err,
    }
}

fn test_error(err: uniwrt_jobs::Error) -> ApiError {
    use uniwrt_jobs::Error;
    match err {
        Error::NoGateway => ApiError::Conflict("No gateway is adopted".to_owned()),
        Error::Observed(_) => ApiError::Conflict(
            "The gateway is in observe mode: turn its management on to test a WAN".to_owned(),
        ),
        Error::NoRevision => ApiError::Conflict("The site has no revision yet".to_owned()),
        err => jobs_error(err),
    }
}
