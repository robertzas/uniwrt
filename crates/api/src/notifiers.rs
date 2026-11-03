//! The notifiers routes, with a session (features/observability.md, Notifications):
//!
//! - `GET /api/v1/notifiers`: every notifier ([`NotifierView`]: its secrets only as which are
//!   set, a webhook's URL as its scheme and host), the settings and the template placeholders
//!   ([`NotifierList`]).
//! - `POST /api/v1/notifiers` and `PUT /api/v1/notifiers/{id}` (admins; audited): a notifier
//!   from a [`NotifierInput`], its secrets sealed in the vault (`notifier/<id>`); secrets left
//!   out are kept.
//! - `DELETE /api/v1/notifiers/{id}` (admins; audited), with its secrets.
//! - `POST /api/v1/notifiers/test` (admins): sends a test through a notifier as given (with a
//!   stored one's secrets when it names its `id`), at once; 502 with why when it fails.
//! - `PUT /api/v1/notifications/settings` (admins; audited): [`NotificationSettings`].

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use uniwrt_core::NotifierId;
use uniwrt_model::{
    notify::{NotificationSettings, Notifier, NotifierConfig, NotifierSpec, WebhookFormat},
    user::AuditAction,
};
use uniwrt_notify::{Header, PLACEHOLDERS, Secrets, check_url, validate};
use uniwrt_store::{NotifierRepo as _, SettingsRepo as _, Vault, VaultRepo as _};

use crate::{
    Api, ApiError, ApiResult, Client, CurrentUser,
    alerts::{admin, audit},
};

/// The most notifiers a controller keeps.
const MAX_NOTIFIERS: usize = 20;

/// A notifier as users see it: its secrets only as which are set.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NotifierView {
    #[serde(flatten)]
    pub notifier: Notifier,
    pub secrets: SecretsView,
}

/// Which secrets a notifier has.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct SecretsView {
    /// A webhook's URL, as its scheme and host (`https://discord.com`).
    pub url: Option<String>,
    pub token: bool,
    pub password: bool,
    /// The names of a webhook's headers.
    pub headers: Vec<String>,
}

impl From<&Secrets> for SecretsView {
    fn from(secrets: &Secrets) -> Self {
        Self {
            url: secrets.url_hint(),
            token: secrets.token.is_some(),
            password: secrets.password.is_some(),
            headers: secrets.headers.iter().map(|h| h.name.clone()).collect(),
        }
    }
}

/// A placeholder of custom webhook templates.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Placeholder {
    pub name: String,
    /// Written unquoted (`{{node}}`); `null` when the message has none.
    pub number: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct NotifierList {
    pub notifiers: Vec<NotifierView>,
    pub settings: NotificationSettings,
    pub placeholders: Vec<Placeholder>,
}

/// A notifier as users set it.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct NotifierInput {
    #[serde(flatten)]
    pub config: NotifierConfig,
    #[serde(default)]
    pub secrets: SecretsInput,
}

/// Secrets to change: one left out is kept, `null` removes it, a value replaces it.
#[allow(clippy::option_option)] // absent, null or a value: TypeScript's `?: string | null`
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct SecretsInput {
    #[serde(default, deserialize_with = "patch")]
    #[ts(optional)]
    pub url: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch")]
    #[ts(optional)]
    pub token: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch")]
    #[ts(optional)]
    pub password: Option<Option<String>>,
    /// Replaces the headers; a header without a value keeps the one stored under its name.
    #[serde(default)]
    #[ts(optional)]
    pub headers: Option<Vec<HeaderInput>>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct HeaderInput {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// A field present (`Some`), with its value or `null`.
#[allow(clippy::option_option)]
fn patch<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

/// A test of a notifier as given; with `id`, a stored one's secrets fill in those left out.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct NotifierTest {
    #[serde(default)]
    pub id: Option<NotifierId>,
    #[serde(flatten)]
    pub notifier: NotifierInput,
}

pub(crate) async fn list(State(api): State<Api>, _: CurrentUser) -> ApiResult<Json<NotifierList>> {
    let (_, vault) = api.notify()?;
    let mut notifiers = Vec::new();
    for notifier in api.store().notifiers().await? {
        let secrets = stored_secrets(vault, notifier.id).await?;
        notifiers.push(NotifierView {
            secrets: SecretsView::from(&secrets),
            notifier,
        });
    }
    Ok(Json(NotifierList {
        notifiers,
        settings: settings(&api).await?,
        placeholders: PLACEHOLDERS
            .iter()
            .map(|(name, number)| Placeholder {
                name: (*name).to_owned(),
                number: *number,
            })
            .collect(),
    }))
}

pub(crate) async fn create(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<NotifierInput>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<NotifierView>)> {
    admin(&user, "set up notifications")?;
    let Json(input) = body?;
    let (notify, vault) = api.notify()?;
    if api.store().notifiers().await?.len() >= MAX_NOTIFIERS {
        return Err(ApiError::Conflict(format!(
            "A controller has at most {MAX_NOTIFIERS} notifiers"
        )));
    }
    let (config, secrets) = prepare(input, &Secrets::default())?;
    let notifier = api.store().create_notifier(config).await?;
    put_secrets(vault, notifier.id, &secrets).await?;
    reload(notify).await;
    let detail = format!(
        "created {} notifier {} {:?}",
        notifier.config.spec.kind(),
        notifier.id,
        notifier.config.name
    );
    audit(&api, &user, &client, AuditAction::Notifier, detail).await?;
    let secrets = SecretsView::from(&secrets);
    Ok((
        StatusCode::CREATED,
        Json(NotifierView { notifier, secrets }),
    ))
}

pub(crate) async fn update(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
    body: Result<Json<NotifierInput>, JsonRejection>,
) -> ApiResult<Json<NotifierView>> {
    admin(&user, "set up notifications")?;
    let Json(input) = body?;
    let (notify, vault) = api.notify()?;
    let id = NotifierId::new(id);
    if api.store().notifier(id).await?.is_none() {
        return Err(not_found(id));
    }
    let (config, secrets) = prepare(input, &stored_secrets(vault, id).await?)?;
    let notifier = api
        .store()
        .update_notifier(id, config)
        .await?
        .ok_or_else(|| not_found(id))?;
    put_secrets(vault, id, &secrets).await?;
    reload(notify).await;
    let detail = format!("edited notifier {id} {:?}", notifier.config.name);
    audit(&api, &user, &client, AuditAction::Notifier, detail).await?;
    let secrets = SecretsView::from(&secrets);
    Ok(Json(NotifierView { notifier, secrets }))
}

pub(crate) async fn delete(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    admin(&user, "set up notifications")?;
    let (notify, vault) = api.notify()?;
    let id = NotifierId::new(id);
    if !api.store().delete_notifier(id).await? {
        return Err(not_found(id));
    }
    vault.delete_secret(&Secrets::vault_name(id)).await?;
    reload(notify).await;
    audit(
        &api,
        &user,
        &client,
        AuditAction::Notifier,
        format!("deleted notifier {id}"),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn test(
    State(api): State<Api>,
    user: CurrentUser,
    body: Result<Json<NotifierTest>, JsonRejection>,
) -> ApiResult<StatusCode> {
    admin(&user, "test notifiers")?;
    let Json(test) = body?;
    let (notify, vault) = api.notify()?;
    let stored = match test.id {
        Some(id) => {
            if api.store().notifier(id).await?.is_none() {
                return Err(not_found(id));
            }
            stored_secrets(vault, id).await?
        }
        None => Secrets::default(),
    };
    let (config, secrets) = prepare(test.notifier, &stored)?;
    let link_url = settings(&api).await?.link_url;
    notify
        .test(&config.spec, &secrets, link_url.as_deref())
        .await
        .map_err(|err| ApiError::BadGateway(format!("The test failed: {err}")))?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn put_settings(
    State(api): State<Api>,
    user: CurrentUser,
    client: Client,
    body: Result<Json<NotificationSettings>, JsonRejection>,
) -> ApiResult<Json<NotificationSettings>> {
    admin(&user, "set up notifications")?;
    let Json(mut settings) = body?;
    let (notify, _) = api.notify()?;
    settings.link_url = settings
        .link_url
        .map(|url| url.trim().trim_end_matches('/').to_owned())
        .filter(|url| !url.is_empty());
    if let Some(url) = &settings.link_url {
        check_url(url).map_err(|why| ApiError::BadRequest(format!("The address {why}")))?;
    }
    let value =
        serde_json::to_value(&settings).map_err(|err| ApiError::Internal(err.to_string()))?;
    api.store()
        .put_setting(NotificationSettings::KEY, value)
        .await?;
    reload(notify).await;
    let detail = format!("notification links to {:?}", settings.link_url);
    audit(&api, &user, &client, AuditAction::Notifier, detail).await?;
    Ok(Json(settings))
}

async fn settings(api: &Api) -> ApiResult<NotificationSettings> {
    Ok(
        match api.store().setting(NotificationSettings::KEY).await? {
            Some(value) => serde_json::from_value(value).unwrap_or_default(),
            None => NotificationSettings::default(),
        },
    )
}

fn not_found(id: NotifierId) -> ApiError {
    ApiError::NotFound(format!("There is no notifier {id}"))
}

async fn stored_secrets(vault: &Vault, id: NotifierId) -> ApiResult<Secrets> {
    match vault.secret(&Secrets::vault_name(id)).await? {
        Some(sealed) => {
            Secrets::from_json(&sealed).map_err(|err| ApiError::Internal(err.to_string()))
        }
        None => Ok(Secrets::default()),
    }
}

async fn put_secrets(vault: &Vault, id: NotifierId, secrets: &Secrets) -> ApiResult<()> {
    let name = Secrets::vault_name(id);
    if secrets.is_empty() {
        vault.delete_secret(&name).await?;
    } else {
        vault.put_secret(&name, secrets.to_json()).await?;
    }
    Ok(())
}

async fn reload(notify: &uniwrt_engine::Notify) {
    if let Err(err) = notify.reload().await {
        tracing::warn!(%err, "cannot have the notifications reload");
    }
}

/// The input tidied, its secrets merged into `stored` and kept only where its kind uses them,
/// and checked.
fn prepare(input: NotifierInput, stored: &Secrets) -> ApiResult<(NotifierConfig, Secrets)> {
    let NotifierInput {
        mut config,
        secrets: change,
    } = input;
    trim(&mut config.name);
    config.filter.nodes.sort_unstable();
    config.filter.nodes.dedup();
    let blank = |value: &mut Option<String>| {
        if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
            *value = None;
        }
    };
    match &mut config.spec {
        NotifierSpec::Ntfy(ntfy) => {
            trim(&mut ntfy.server);
            while ntfy.server.ends_with('/') {
                ntfy.server.pop();
            }
            trim(&mut ntfy.topic);
            blank(&mut ntfy.username);
        }
        NotifierSpec::Webhook(webhook) => {
            if webhook.format != WebhookFormat::Custom {
                webhook.template = None;
            }
        }
        NotifierSpec::Email(email) => {
            trim(&mut email.host);
            trim(&mut email.from);
            blank(&mut email.username);
            let mut to = Vec::new();
            for address in email.to.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
                if !to.iter().any(|t: &String| t.eq_ignore_ascii_case(address)) {
                    to.push(address.to_owned());
                }
            }
            email.to = to;
        }
    }
    let mut secrets = stored.clone();
    let set = |field: &mut Option<String>, change: Option<Option<String>>| {
        if let Some(change) = change {
            *field = change.filter(|value| !value.is_empty());
        }
    };
    set(&mut secrets.url, change.url);
    set(&mut secrets.token, change.token);
    set(&mut secrets.password, change.password);
    if let Some(headers) = change.headers {
        secrets.headers = headers
            .into_iter()
            .filter_map(|header| {
                let name = header.name.trim().to_owned();
                let value = header.value.or_else(|| {
                    stored
                        .headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case(&name))
                        .map(|h| h.value.clone())
                })?;
                Some(Header { name, value })
            })
            .collect();
    }
    // Only what the kind uses is kept.
    let mut kept = Secrets::default();
    match &config.spec {
        NotifierSpec::Ntfy(ntfy) if ntfy.username.is_some() => {
            kept.password = secrets.password.take();
        }
        NotifierSpec::Ntfy(_) => kept.token = secrets.token.take(),
        NotifierSpec::Webhook(_) => {
            kept.url = secrets.url.take();
            kept.headers = std::mem::take(&mut secrets.headers);
        }
        NotifierSpec::Email(email) if email.username.is_some() => {
            kept.password = secrets.password.take();
        }
        NotifierSpec::Email(_) => {}
    }
    let secrets = kept;
    validate(&config, &secrets).map_err(ApiError::BadRequest)?;
    Ok((config, secrets))
}

/// Without the white space around it.
fn trim(text: &mut String) {
    let trimmed = text.trim();
    if trimmed.len() != text.len() {
        *text = trimmed.to_owned();
    }
}

#[cfg(test)]
#[path = "notifiers_tests.rs"]
mod tests;
