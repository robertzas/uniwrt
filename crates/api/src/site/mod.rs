//! The site's desired state, with a session (features/lifecycle.md, Desired state & revisions;
//! D-065):
//!
//! - `GET /api/v1/site`: the latest revision's document ([`SiteView`]), its secrets only as
//!   references and which are set, with the warnings it raises.
//! - `PUT /api/v1/site` (admins; audited): a [`SiteSave`] becomes the next revision. `base` must
//!   still be the latest (else 409); `secrets` sets (a value) or clears (`null`) secrets by path,
//!   each new value sealed in the vault under a new name; secrets left out keep the references
//!   the document holds. A document with errors is refused with 422 and its `issues`; one
//!   that changes nothing saves no revision.
//! - `POST /api/v1/site/secrets/reveal` (admins; audited): a secret's value ([`RevealSecret`]).
//! - The history, under `v1/revisions`: see [`revisions`].
//!
//! [`ensure_first_revision`] gives a new site its first revision when the API starts: the
//! defaults, or a [`FirstSite`] (demo mode's, from the simulated network).

pub(crate) mod revisions;

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use ring::rand::{SecureRandom as _, SystemRandom};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uniwrt_core::{Ipv6Net, NodeId, RevisionId};
use uniwrt_model::{
    site::{
        SCHEMA_VERSION, SecretRef, SiteConfig, check_secret,
        revision::{NewRevision, RevisionKind, RevisionMeta},
        validate::{Issue, Severity, has_errors, validate},
    },
    user::AuditAction,
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, SealedSecret, Store, VaultRepo as _};
use zeroize::Zeroizing;

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

/// The longest message a revision takes.
const MAX_MESSAGE: usize = 500;

/// The site's document as the latest revision holds it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SiteView {
    pub revision: RevisionMeta,
    pub config: SiteConfig,
    /// Every slot that can hold a secret, and whether it does.
    pub secrets: Vec<SecretState>,
    /// The document's warnings (a saved document has no errors).
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SecretState {
    /// Such as `wlans/home/passphrase`.
    pub path: String,
    pub set: bool,
}

/// A save: the edited document, the revision it was edited from, and the secrets to change.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SiteSave {
    pub base: RevisionId,
    /// Why, for the history; may be empty.
    #[serde(default)]
    #[ts(optional)]
    pub message: Option<String>,
    pub config: SiteConfig,
    /// By path: a value sets the secret, `null` clears it.
    #[serde(default)]
    #[ts(optional)]
    pub secrets: Option<BTreeMap<String, Option<String>>>,
}

/// Which secret to show: by path, in a revision (the latest when left out).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RevealSecret {
    #[serde(default)]
    #[ts(optional)]
    pub revision: Option<RevisionId>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RevealedSecret {
    pub value: String,
}

pub(crate) async fn get(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<SiteView>> {
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    Ok(Json(view(&api, head.meta, head.config).await?))
}

pub(crate) async fn put(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<SiteSave>, JsonRejection>,
) -> ApiResult<Json<SiteView>> {
    admin(&user, "change the site")?;
    let Json(save) = body?;
    let message = message(save.message)?;
    let mut config = save.config;
    if config.schema != SCHEMA_VERSION {
        return Err(ApiError::BadRequest(format!(
            "The document is schema {}; this controller writes {SCHEMA_VERSION}",
            config.schema
        )));
    }
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let sealed = secrets(&api, &mut config, save.secrets.unwrap_or_default())?;
    if sealed.is_empty() && config == head.config {
        return Ok(Json(view(&api, head.meta, head.config).await?));
    }
    let issues = checked(&api, &config).await?;
    let new = NewRevision {
        base: Some(save.base),
        kind: RevisionKind::Edit,
        restored_from: None,
        author_id: Some(user.user.id),
        author: Some(user.user.username.clone()),
        message,
        config: config.clone(),
    };
    let meta = api.store().commit_revision(new, sealed).await?;
    api.config_changed();
    let detail = format!("saved revision {} ({} changes)", meta.id, meta.changes);
    audit(&api, &user, &client, AuditAction::Site, detail).await?;
    Ok(Json(SiteView {
        secrets: secret_states(&config),
        revision: meta,
        config,
        issues,
    }))
}

pub(crate) async fn reveal(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<RevealSecret>, JsonRejection>,
) -> ApiResult<Json<RevealedSecret>> {
    admin(&user, "view secrets")?;
    let Json(RevealSecret { revision, path }) = body?;
    let vault = api.vault()?;
    let revision = match revision {
        Some(id) => api
            .store()
            .revision(id)
            .await?
            .ok_or_else(|| revisions::missing(id))?,
        None => api.store().head_revision().await?.ok_or_else(no_site)?,
    };
    let name = revision.config.secret(&path).ok_or_else(|| {
        ApiError::NotFound(format!(
            "Revision {} sets no secret {path}",
            revision.meta.id
        ))
    })?;
    let value = vault
        .secret(name.name())
        .await?
        .ok_or_else(|| ApiError::Internal(format!("the vault lacks {}", name.name())))?;
    let value = String::from_utf8(value.to_vec())
        .map_err(|_| ApiError::Internal(format!("{} is not UTF-8", name.name())))?;
    let detail = format!("viewed {path} of revision {}", revision.meta.id);
    audit(&api, &user, &client, AuditAction::RevealSecret, detail).await?;
    Ok(Json(RevealedSecret { value }))
}

/// Seals the secrets a save sets and points their slots at them; clears the ones it clears.
fn secrets(
    api: &Api,
    config: &mut SiteConfig,
    secrets: BTreeMap<String, Option<String>>,
) -> ApiResult<Vec<SealedSecret>> {
    let mut sealed = Vec::new();
    for (path, value) in secrets {
        let value = value.map(Zeroizing::new);
        let slot = config
            .secret_mut(&path)
            .ok_or_else(|| ApiError::BadRequest(format!("The document has no secret at {path}")))?;
        let Some(value) = value else {
            *slot = None;
            continue;
        };
        if let Err(why) = check_secret(&path, &value) {
            let issue = Issue {
                path,
                severity: Severity::Error,
                message: why.to_owned(),
            };
            return Err(ApiError::Invalid(why.to_owned(), vec![issue]));
        }
        let secret = api.vault()?.seal_config_secret(value.as_bytes())?;
        *slot = Some(secret.reference().clone());
        sealed.push(secret);
    }
    Ok(sealed)
}

/// The document's warnings, or 422 with its issues when it has errors.
pub(crate) async fn checked(api: &Api, config: &SiteConfig) -> ApiResult<Vec<Issue>> {
    let issues = validate(config, &adopted(api.store()).await?);
    if has_errors(&issues) {
        let errors = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .count();
        let detail = if errors == 1 {
            "The configuration has an error".to_owned()
        } else {
            format!("The configuration has {errors} errors")
        };
        return Err(ApiError::Invalid(detail, issues));
    }
    Ok(issues)
}

pub(crate) async fn view(
    api: &Api,
    revision: RevisionMeta,
    config: SiteConfig,
) -> ApiResult<SiteView> {
    let issues = validate(&config, &adopted(api.store()).await?);
    Ok(SiteView {
        secrets: secret_states(&config),
        revision,
        config,
        issues,
    })
}

fn secret_states(config: &SiteConfig) -> Vec<SecretState> {
    config
        .secrets()
        .into_iter()
        .map(|slot| SecretState {
            set: slot.value.is_some(),
            path: slot.path,
        })
        .collect()
}

async fn adopted(store: &Store) -> ApiResult<BTreeSet<NodeId>> {
    Ok(store
        .nodes()
        .await?
        .into_iter()
        .map(|node| node.id)
        .collect())
}

/// A revision's message: trimmed, at most [`MAX_MESSAGE`] characters.
pub(crate) fn message(message: Option<String>) -> ApiResult<String> {
    let message = message.unwrap_or_default();
    let message = message.trim();
    if message.chars().count() > MAX_MESSAGE {
        return Err(ApiError::BadRequest(format!(
            "A message is at most {MAX_MESSAGE} characters"
        )));
    }
    Ok(message.to_owned())
}

fn no_site() -> ApiError {
    ApiError::Unavailable("The site has no configuration yet".to_owned())
}

/// A new site's first revision when it isn't the defaults, such as demo mode's (the import of
/// the simulated fleet): the document, why, and its secrets' values under the placeholder
/// references the document holds, each sealed under a name of its own.
#[derive(Debug, Clone)]
pub struct FirstSite {
    pub config: SiteConfig,
    /// How it came about, such as an import.
    pub kind: RevisionKind,
    pub message: String,
    pub secrets: BTreeMap<SecretRef, Zeroizing<String>>,
}

/// Commits a new site's first revision when there is no revision yet: `first`, checked as a
/// save is, or else the defaults (with a random ULA).
pub(crate) async fn ensure_first_revision(api: &Api, first: Option<FirstSite>) -> ApiResult<()> {
    let store = api.store();
    if store.head_revision().await?.is_some() {
        return Ok(());
    }
    let (config, kind, message, sealed) = match first {
        Some(FirstSite {
            mut config,
            kind,
            message,
            secrets: values,
        }) => {
            let sealed = seal_found(api, &mut config, values)?;
            checked(api, &config).await?;
            (config, kind, message, sealed)
        }
        None => (
            SiteConfig::defaults(Some(random_ula()?)),
            RevisionKind::Defaults,
            "The defaults of a new site".to_owned(),
            Vec::new(),
        ),
    };
    let new = NewRevision {
        base: None,
        kind,
        restored_from: None,
        author_id: None,
        author: None,
        message,
        config,
    };
    match store.commit_revision(new, sealed).await {
        Ok(_) => {
            api.config_changed();
            Ok(())
        }
        Err(uniwrt_store::Error::NotLatest { .. }) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Seals the secrets found for `config`, held under placeholder references, each checked as
/// its slot requires, and points the slots at their new names.
fn seal_found(
    api: &Api,
    config: &mut SiteConfig,
    found: BTreeMap<SecretRef, Zeroizing<String>>,
) -> ApiResult<Vec<SealedSecret>> {
    let paths: BTreeMap<SecretRef, String> = config
        .secrets()
        .into_iter()
        .filter_map(|slot| Some((slot.value?.clone(), slot.path)))
        .collect();
    let mut sealed = Vec::new();
    for (placeholder, value) in found {
        let Some(path) = paths.get(&placeholder) else {
            continue;
        };
        if let Err(why) = check_secret(path, &value) {
            let issue = Issue {
                path: path.clone(),
                severity: Severity::Error,
                message: why.to_owned(),
            };
            return Err(ApiError::Invalid(why.to_owned(), vec![issue]));
        }
        let secret = api.vault()?.seal_config_secret(value.as_bytes())?;
        config.replace_secret(&placeholder, secret.reference());
        sealed.push(secret);
    }
    Ok(sealed)
}

/// A random unique local prefix (RFC 4193): `fd` and a 40-bit global ID, as a /48.
fn random_ula() -> ApiResult<Ipv6Net> {
    let mut id = [0_u8; 5];
    SystemRandom::new()
        .fill(&mut id)
        .map_err(|_| ApiError::Internal("the random number generator failed".to_owned()))?;
    let mut octets = [0_u8; 16];
    octets[0] = 0xfd;
    octets[1..6].copy_from_slice(&id);
    Ipv6Net::new(octets.into(), 48).map_err(|err| ApiError::Internal(err.to_string()))
}

#[cfg(test)]
mod tests;
