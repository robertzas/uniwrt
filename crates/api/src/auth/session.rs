//! Login sessions: a random 256-bit token in the `__Host-uniwrt-session` cookie (`HttpOnly`,
//! Secure, SameSite=Strict, the whole site), stored only as its SHA-256. A session ends
//! [`ABSOLUTE`] after login, or after [`IDLE`] without a request; [`CurrentUser`] resolves it.

use std::time::Duration;

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, HeaderValue, header, request::Parts},
};
use data_encoding::BASE64URL_NOPAD;
use ring::{
    digest,
    rand::{SecureRandom as _, SystemRandom},
};
use uniwrt_core::Timestamp;
use uniwrt_model::user::User;
use uniwrt_store::{NewSession, SessionRepo as _};
use zeroize::Zeroizing;

use crate::{Api, ApiError, Client};

/// `__Host-`: the browser accepts the cookie only from a secure origin, for the whole site and
/// without a `Domain`, so no other host can set or read it.
pub(crate) const COOKIE: &str = "__Host-uniwrt-session";
/// However active, a session ends this long after login.
pub(crate) const ABSOLUTE: Duration = Duration::from_hours(30 * 24);
/// A session unused this long ends.
pub(crate) const IDLE: Duration = Duration::from_hours(7 * 24);
/// Requests record their session's use at most this often, to spare the writer.
const TOUCH_EVERY: Duration = Duration::from_mins(5);

/// A session token as the browser holds it.
pub(crate) struct Token(Zeroizing<String>);

impl Token {
    pub(crate) fn generate() -> Result<Self, ApiError> {
        let mut bytes = Zeroizing::new([0; 32]);
        SystemRandom::new()
            .fill(bytes.as_mut())
            .map_err(|_| ApiError::Internal("the random number generator failed".to_owned()))?;
        Ok(Self(Zeroizing::new(BASE64URL_NOPAD.encode(bytes.as_ref()))))
    }

    /// The cookie that hands it to the browser.
    pub(crate) fn cookie(&self) -> HeaderValue {
        let cookie = Zeroizing::new(format!(
            "{COOKIE}={}; Path=/; Max-Age={}; Secure; HttpOnly; SameSite=Strict",
            self.0.as_str(),
            ABSOLUTE.as_secs()
        ));
        HeaderValue::from_str(&cookie).unwrap_or_else(|_| clear_cookie())
    }

    pub(crate) fn sha256(&self) -> [u8; 32] {
        sha256(&self.0)
    }
}

fn sha256(token: &str) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, token.as_bytes());
    let mut hash = [0; 32];
    hash.copy_from_slice(digest.as_ref());
    hash
}

/// A cookie that removes the session cookie.
pub(crate) fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static(
        "__Host-uniwrt-session=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict",
    )
}

/// Starts a session for `user` and returns its token.
pub(crate) async fn start(
    api: &Api,
    user: &User,
    client: &Client,
    headers: &HeaderMap,
) -> Result<Token, ApiError> {
    let token = Token::generate()?;
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|agent| agent.to_str().ok())
        .map(|agent| agent.chars().take(256).collect());
    api.store()
        .create_session(NewSession {
            token_sha256: token.sha256(),
            user: user.id,
            expires_at: api.now().saturating_add(ABSOLUTE),
            ip: Some(client.ip.to_string()),
            user_agent,
        })
        .await?;
    Ok(token)
}

/// The SHA-256 of the session token the request carries, if it carries one.
pub(crate) fn presented(headers: &HeaderMap) -> Option<[u8; 32]> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|cookie| {
            let (name, value) = cookie.trim().split_once('=')?;
            (name == COOKIE && !value.is_empty()).then(|| sha256(value))
        })
}

/// The user of the request's session, which must be valid.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user: User,
}

impl FromRequestParts<Api> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, api: &Api) -> Result<Self, Self::Rejection> {
        let unauthorized = || ApiError::Unauthorized("Log in first".to_owned());
        let token_sha256 = presented(&parts.headers).ok_or_else(unauthorized)?;
        current(api, token_sha256).await?.ok_or_else(unauthorized)
    }
}

/// The user of the session with this token hash, if it is valid; an ended session is deleted.
pub(crate) async fn current(
    api: &Api,
    token_sha256: [u8; 32],
) -> Result<Option<CurrentUser>, ApiError> {
    let Some(session) = api.store().session(token_sha256).await? else {
        return Ok(None);
    };
    let now = api.now();
    if !is_live(session.expires_at, session.last_seen_at, now) {
        api.store().delete_session(token_sha256).await?;
        return Ok(None);
    }
    if now.saturating_duration_since(session.last_seen_at) >= TOUCH_EVERY {
        api.store().touch_session(token_sha256).await?;
    }
    Ok(Some(CurrentUser { user: session.user }))
}

fn is_live(expires_at: Timestamp, last_seen_at: Timestamp, now: Timestamp) -> bool {
    now < expires_at && now.saturating_duration_since(last_seen_at) < IDLE
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::{HeaderMap, HeaderValue, header};
    use uniwrt_core::Timestamp;

    use super::{ABSOLUTE, IDLE, Token, is_live, presented, sha256};

    #[test]
    fn tokens_are_256_random_bits_in_a_host_only_secure_cookie() {
        let token = Token::generate().unwrap();
        assert_eq!(token.0.len(), 43);
        assert_ne!(Token::generate().unwrap().0.as_str(), token.0.as_str());
        let cookie = token.cookie();
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.starts_with(&format!("__Host-uniwrt-session={}; ", token.0.as_str())));
        for attribute in [
            "Path=/",
            "Max-Age=2592000",
            "Secure",
            "HttpOnly",
            "SameSite=Strict",
        ] {
            assert!(cookie.contains(attribute), "{attribute}");
        }
        assert!(!cookie.contains("Domain"));
    }

    #[test]
    fn the_token_is_found_among_other_cookies() {
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, HeaderValue::from_static("theme=dark"));
        assert_eq!(presented(&headers), None);
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; __Host-uniwrt-session=tok; b=2"),
        );
        assert_eq!(presented(&headers), Some(sha256("tok")));
        let mut empty = HeaderMap::new();
        empty.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-uniwrt-session="),
        );
        assert_eq!(presented(&empty), None);
    }

    #[test]
    fn sessions_end_when_idle_or_at_their_absolute_end() {
        let login = Timestamp::from_unix_secs(1_793_318_400);
        let end = login.saturating_add(ABSOLUTE);
        let at = |d: Duration| login.saturating_add(d);
        assert!(is_live(end, login, login));
        assert!(is_live(
            end,
            login,
            at(IDLE.saturating_sub(Duration::from_secs(1)))
        ));
        assert!(!is_live(end, login, at(IDLE)));
        let recent = end.saturating_sub(Duration::from_secs(60));
        assert!(is_live(
            end,
            recent,
            end.saturating_sub(Duration::from_secs(1))
        ));
        assert!(!is_live(end, recent, end));
    }
}
