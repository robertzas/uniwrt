//! A notifier's secrets, sealed in the vault as one JSON value under `notifier/<id>`
//! (features/security.md, Secrets vault): a webhook's URL (services put their tokens in it) and
//! headers, an ntfy access token or password, an SMTP password. They are zeroized when dropped
//! and never printed.

use std::fmt;

use serde::{Deserialize, Serialize};
use uniwrt_core::NotifierId;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// What [`Secrets::from_json`] refuses.
#[derive(Debug, thiserror::Error)]
#[error("a notifier's secrets cannot be read: {0}")]
pub struct SecretsError(String);

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Secrets {
    /// A webhook's URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// ntfy's access token, when it has no username.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The password of ntfy's or SMTP's username.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// A webhook's extra headers, such as `Authorization`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<Header>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Secrets {
    /// Its name in the vault.
    pub fn vault_name(id: NotifierId) -> String {
        format!("notifier/{id}")
    }

    pub fn to_json(&self) -> Zeroizing<Vec<u8>> {
        // Serializing strings into a Vec cannot fail.
        Zeroizing::new(serde_json::to_vec(self).unwrap_or_default())
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, SecretsError> {
        serde_json::from_slice(bytes).map_err(|err| SecretsError(err.to_string()))
    }

    pub fn is_empty(&self) -> bool {
        self.url.is_none()
            && self.token.is_none()
            && self.password.is_none()
            && self.headers.is_empty()
    }

    /// What may be shown of a webhook's URL: its scheme and host (`https://discord.com`).
    pub fn url_hint(&self) -> Option<String> {
        let url = self.url.as_deref()?;
        let (scheme, rest) = url.split_once("://")?;
        let host = rest.split(['/', '?', '#']).next()?;
        let host = host.rsplit('@').next()?;
        Some(format!("{scheme}://{host}"))
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("url", &self.url.as_ref().map(|_| "…"))
            .field("token", &self.token.as_ref().map(|_| "…"))
            .field("password", &self.password.as_ref().map(|_| "…"))
            .field("headers", &self.headers.len())
            .finish()
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Header({}: …)", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::Secrets;

    #[test]
    fn round_trips_and_hides() {
        let mut secrets = Secrets::default();
        secrets.url = Some("https://user:pw@discord.com/api/webhooks/1/abc?x=1".to_owned());
        assert_eq!(secrets.url_hint().as_deref(), Some("https://discord.com"));
        let json = secrets.to_json();
        assert_eq!(Secrets::from_json(&json).unwrap(), secrets);
        assert!(!format!("{secrets:?}").contains("abc"));
        assert!(Secrets::default().is_empty());
    }
}
