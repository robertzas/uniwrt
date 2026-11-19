//! The site as a file (features/lifecycle.md, Backups: Site export/import; D-130), with a
//! session:
//!
//! - `GET /api/v1/site/export`: the latest revision as a [`SiteFile`] without its secrets, as
//!   an attachment (pretty-printed, so that it diffs well under version control).
//! - `POST /api/v1/site/export` with `{"passphrase"}` ([`SiteExportBody`]; admins, audited):
//!   the same with its secrets, an armored age file (one scrypt stanza) of path → value. A
//!   passphrase has at least [`MIN_PASSPHRASE`] characters.
//! - `POST /api/v1/site/import/preview` with `{"file", "passphrase"?}` ([`SiteImportBody`];
//!   admins): what importing the file onto the latest revision would save
//!   ([`SiteImportPreview`]): its changes, issues, the node mapping by MAC, and where each
//!   secret comes from. A file whose secrets aren't opened (no passphrase) imports without
//!   them; a wrong passphrase is refused. The controller holds the newest preview for
//!   [`HELD`], apart from the brownfield import's; values are sealed only on accept.
//! - `POST /api/v1/site/import/accept` with `{"id", "message"?, "secrets"?}`
//!   ([`SiteImportAccept`]; admins, audited): saves the held preview as the next revision
//!   (kind `file`), with values for the secrets the file left missing: 404 when it is not the
//!   held one (or expired), 409 once another revision was saved since, 422 while its document
//!   has errors (it stays held). One that changes nothing saves nothing.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::Value;
use ts_rs::TS;
use uniwrt_core::Timestamp;
use uniwrt_model::{
    controller_backup::MIN_PASSPHRASE,
    node::Node,
    site::{
        SiteConfig,
        diff::diff,
        export::{self, FileError, Head, SiteFile, SiteImport, SiteImportPreview},
        revision::{NewRevision, RevisionKind},
        validate::{Severity, has_errors, validate},
    },
    user::AuditAction,
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, VaultRepo as _, age::armor};
use zeroize::Zeroizing;

use super::{SiteView, ensure_first_revision, message, no_site, seal_found, view};
use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
    import::{HELD, random_id},
    vpn,
};

/// scrypt's work factor for exported secrets: age's own, cheap in tests.
const WORK_FACTOR: u8 = if cfg!(test) {
    4
} else {
    uniwrt_store::age::LOG_N
};

/// An export with the secrets.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SiteExportBody {
    pub passphrase: String,
}

/// A file to import, and the passphrase of its secrets.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SiteImportBody {
    /// The file as exported ([`SiteFile`]).
    #[ts(type = "unknown")]
    pub file: Value,
    #[serde(default)]
    #[ts(optional)]
    pub passphrase: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SiteImportAccept {
    /// The preview reviewed ([`SiteImportPreview::id`]).
    pub id: String,
    #[serde(default)]
    #[ts(optional)]
    pub message: Option<String>,
    /// Values for secrets the file left missing, by path.
    #[serde(default)]
    #[ts(optional)]
    pub secrets: Option<BTreeMap<String, String>>,
}

/// The newest file import's preview, until accepted, replaced or expired.
pub(crate) struct Held {
    at: Timestamp,
    import: SiteImport,
}

pub(crate) async fn export_plain(State(api): State<Api>, _: CurrentUser) -> ApiResult<Response> {
    let (file, _) = exported(&api, None).await?;
    download(&file)
}

pub(crate) async fn export_secret(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<SiteExportBody>, JsonRejection>,
) -> ApiResult<Response> {
    admin(&user, "export the site's secrets")?;
    let Json(SiteExportBody { passphrase }) = body?;
    let passphrase = Zeroizing::new(passphrase);
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(ApiError::BadRequest(format!(
            "A passphrase has at least {MIN_PASSPHRASE} characters"
        )));
    }
    let (file, count) = exported(&api, Some(passphrase)).await?;
    let detail = format!(
        "exported revision {} with its {count} secrets",
        file.revision
    );
    audit(&api, &user, &client, AuditAction::RevealSecret, detail).await?;
    download(&file)
}

/// The latest revision as a file, with its secrets sealed for `passphrase` if given, and how
/// many it holds.
async fn exported(
    api: &Api,
    passphrase: Option<Zeroizing<String>>,
) -> ApiResult<(SiteFile, usize)> {
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let adopted = api.store().nodes().await?;
    let mut count = 0;
    let secrets = match passphrase {
        Some(passphrase) => {
            let vault = api.vault()?;
            let mut values = BTreeMap::new();
            for (path, name) in export::exported_secrets(&head.config, &adopted) {
                let value = vault.secret(name.name()).await?.ok_or_else(|| {
                    ApiError::Internal(format!("the vault lacks {}", name.name()))
                })?;
                let value =
                    Zeroizing::new(String::from_utf8(value.to_vec()).map_err(|_| {
                        ApiError::Internal(format!("{} is not UTF-8", name.name()))
                    })?);
                values.insert(path, value);
            }
            count = values.len();
            let borrowed: BTreeMap<&str, &str> = values
                .iter()
                .map(|(path, value)| (path.as_str(), value.as_str()))
                .collect();
            let plain = Zeroizing::new(
                serde_json::to_vec(&borrowed).map_err(|err| ApiError::Internal(err.to_string()))?,
            );
            let sealed = tokio::task::spawn_blocking(move || {
                armor::seal_with(&plain, passphrase.as_bytes(), WORK_FACTOR)
            })
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?
            .map_err(|err| ApiError::Internal(err.to_string()))?;
            Some(sealed)
        }
        None => None,
    };
    let file = SiteFile::new(
        head.meta.id,
        &head.config,
        &adopted,
        api.now(),
        api.build().version,
        secrets,
    )
    .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok((file, count))
}

fn download(file: &SiteFile) -> ApiResult<Response> {
    let mut body =
        serde_json::to_vec_pretty(file).map_err(|err| ApiError::Internal(err.to_string()))?;
    body.push(b'\n');
    let name = format!("uniwrt-site-r{}.json", file.revision);
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        body,
    )
        .into_response())
}

pub(crate) async fn preview(
    State(api): State<Api>,
    user: CurrentUser,
    body: Result<Json<SiteImportBody>, JsonRejection>,
) -> ApiResult<Json<SiteImportPreview>> {
    admin(&user, "import a site file")?;
    let Json(SiteImportBody { file, passphrase }) = body?;
    let file = SiteFile::parse(file).map_err(|err| file_error(&err))?;
    let values = match (&file.secrets, passphrase) {
        (Some(armored), Some(passphrase)) if !passphrase.is_empty() => {
            Some(opened(armored.clone(), Zeroizing::new(passphrase)).await?)
        }
        _ => None,
    };
    ensure_first_revision(&api, None).await?;
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let adopted = api.store().nodes().await?;
    let current = current_values(&api, &head.config, values.as_ref()).await?;
    let at = Head {
        config: &head.config,
        values: &current,
    };
    let mut import = export::import(file, values, at, &adopted).map_err(|err| file_error(&err))?;
    import.preview.id = random_id()?;
    import.preview.base = head.meta.id;
    let preview = import.preview.clone();
    *api.site_imports() = Some(Held {
        at: api.now(),
        import,
    });
    Ok(Json(preview))
}

/// The values an armored blob holds by path.
async fn opened(
    armored: String,
    passphrase: Zeroizing<String>,
) -> ApiResult<BTreeMap<String, Zeroizing<String>>> {
    let plain = tokio::task::spawn_blocking(move || armor::open(&armored, passphrase.as_bytes()))
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?
        .map_err(|err| match err {
            uniwrt_store::Error::Passphrase => {
                ApiError::BadRequest("The passphrase doesn't open the file's secrets".to_owned())
            }
            err => ApiError::BadRequest(format!("The file's secrets don't open: {err}")),
        })?;
    let values: BTreeMap<String, String> = serde_json::from_slice(&plain).map_err(|_| {
        ApiError::BadRequest("The file's secrets aren't an object of paths and values".to_owned())
    })?;
    Ok(values
        .into_iter()
        .map(|(path, value)| (path, Zeroizing::new(value)))
        .collect())
}

/// The values the latest revision holds at the paths the file sets, to compare them.
async fn current_values(
    api: &Api,
    head: &SiteConfig,
    values: Option<&BTreeMap<String, Zeroizing<String>>>,
) -> ApiResult<BTreeMap<String, Zeroizing<String>>> {
    let mut current = BTreeMap::new();
    let Some(values) = values else {
        return Ok(current);
    };
    let vault = api.vault()?;
    for slot in head.secrets() {
        let Some(name) = slot.value else { continue };
        if !values.contains_key(&slot.path) {
            continue;
        }
        // A value that isn't UTF-8 can't equal the file's.
        if let Some(value) = vault.secret(name.name()).await?
            && let Ok(value) = String::from_utf8(value.to_vec())
        {
            current.insert(slot.path, Zeroizing::new(value));
        }
    }
    Ok(current)
}

pub(crate) async fn accept(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<SiteImportAccept>, JsonRejection>,
) -> ApiResult<Json<SiteView>> {
    admin(&user, "import a site file")?;
    let Json(SiteImportAccept {
        id,
        message: text,
        secrets,
    }) = body?;
    let text = message(text)?;
    let mut import = take(&api, &id)?;
    let mut issues = Vec::new();
    for (path, value) in secrets.unwrap_or_default() {
        if let Err(issue) = export::fill_secret(&mut import, &path, Zeroizing::new(value)) {
            issues.push(issue);
        }
    }
    // The file's values that failed their check and weren't given now, then the document's,
    // checked with the VPN keys it lacks made (kept only once it is accepted).
    issues.extend(import.rejected.iter().cloned());
    let head = api.store().head_revision().await?.ok_or_else(no_site)?;
    let mut site = import.site.clone();
    let server = head.config.vpn.wireguard.as_ref();
    let generated = vpn::keys::fill(&api, &mut site, server, &import.values).await?;
    let adopted = api.store().nodes().await?;
    issues.extend(validate(&site, &ids(&adopted)));
    if has_errors(&issues) {
        let errors = issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .count();
        import.preview.issues = issues.clone();
        // Kept: the user can still read it and give what it lacks.
        *api.site_imports() = Some(Held {
            at: api.now(),
            import,
        });
        let detail = if errors == 1 {
            "The import has an error".to_owned()
        } else {
            format!("The import has {errors} errors")
        };
        return Err(ApiError::Invalid(detail, issues));
    }
    if head.meta.id != import.preview.base {
        return Err(ApiError::Conflict(format!(
            "Revision {} was saved since this preview: import the file again",
            head.meta.id
        )));
    }
    let SiteImport {
        values, preview, ..
    } = import;
    let mut sealed = seal_found(&api, &mut site, values)?;
    sealed.extend(generated);
    if sealed.is_empty() && diff(&head.config, &site).is_empty() {
        return Ok(Json(view(&api, head.meta, head.config).await?));
    }
    let message = if text.is_empty() {
        format!(
            "Imported from a file: revision {} of uniwrt {}",
            preview.file.revision, preview.file.uniwrt
        )
    } else {
        text
    };
    let new = NewRevision {
        base: Some(preview.base),
        kind: RevisionKind::File,
        restored_from: None,
        author_id: Some(user.user.id),
        author: Some(user.user.username.clone()),
        message,
        config: site.clone(),
    };
    let meta = api.store().commit_revision(new, sealed).await?;
    api.config_changed();
    let detail = format!(
        "imported revision {} from a file ({} changes)",
        meta.id, meta.changes
    );
    audit(&api, &user, &client, AuditAction::Site, detail).await?;
    Ok(Json(view(&api, meta, site).await?))
}

fn ids(adopted: &[Node]) -> std::collections::BTreeSet<uniwrt_core::NodeId> {
    adopted.iter().map(|node| node.id).collect()
}

/// Takes the held preview named `id`, if it hasn't expired.
fn take(api: &Api, id: &str) -> ApiResult<SiteImport> {
    let mut held = api.site_imports();
    let fresh = held.as_ref().is_some_and(|held| {
        held.import.preview.id == id && api.now().saturating_duration_since(held.at) <= HELD
    });
    match held.take() {
        Some(held) if fresh => Ok(held.import),
        other => {
            *held = other;
            Err(ApiError::NotFound(
                "This preview is no longer held: import the file again".to_owned(),
            ))
        }
    }
}

fn file_error(err: &FileError) -> ApiError {
    ApiError::BadRequest(capitalized(&err.to_string()))
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
