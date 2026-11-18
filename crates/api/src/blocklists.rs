//! DNS filtering's blocklists (features/network.md, Blocklists; D-133):
//!
//! - `GET /api/v1/services/blocklists`: the catalog the settings choose from, each list's
//!   download state, the file compiled for the gateway and where the gateway's copy stands
//!   ([`BlocklistsView`]).
//! - `POST /api/v1/services/blocklists/refresh` (admins): downloads the lists again now
//!   (conditionally); 202 with the view, or 409 while blocking is off in the saved and the
//!   applied revisions. Audited.
//!
//! The settings are the site's (`services.blocking`, `PUT v1/site`); the gateway gets them with
//! an apply, and refreshed lists without one.

use axum::{Json, extract::State, http::StatusCode};
use serde::Serialize;
use ts_rs::TS;
use uniwrt_engine::BlocklistError;
use uniwrt_model::{
    blocklists::BlocklistState,
    site::blocking::{CATALOG, CatalogList},
    user::AuditAction,
};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

/// The catalog, and DNS filtering's state.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BlocklistsView {
    pub catalog: Vec<CatalogList>,
    #[serde(flatten)]
    #[ts(flatten)]
    pub state: BlocklistState,
}

fn view(state: &BlocklistState) -> BlocklistsView {
    BlocklistsView {
        catalog: CATALOG.to_vec(),
        state: state.clone(),
    }
}

pub(crate) async fn get(
    State(api): State<Api>,
    _user: CurrentUser,
) -> ApiResult<Json<BlocklistsView>> {
    Ok(Json(view(&api.blocklist()?.state())))
}

pub(crate) async fn refresh(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
) -> ApiResult<(StatusCode, Json<BlocklistsView>)> {
    admin(&user, "refresh the blocklists")?;
    let blocklist = api.blocklist()?;
    blocklist.refresh().await.map_err(|err| match err {
        BlocklistError::Off => ApiError::Conflict(err.to_string()),
        BlocklistError::Stopped => ApiError::Unavailable(err.to_string()),
    })?;
    let detail = "refreshed DNS filtering's lists".to_owned();
    audit(&api, &user, &client, AuditAction::Services, detail).await?;
    Ok((StatusCode::ACCEPTED, Json(view(&blocklist.state()))))
}

#[cfg(test)]
#[path = "blocklists_tests.rs"]
mod tests;
