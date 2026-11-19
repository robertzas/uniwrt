//! `tailscale`: the gateway's Tailscale daemon as the controller wants it
//! (`node-agent/tailscale.uc`; D-140), through the `tailscale` CLI: its prefs, logging it in
//! (with an auth key, or by the login URL it hands out) or out, stopping and starting it.

use std::fmt;

use serde::{Deserialize, Serialize};
use uniwrt_model::{facts::TailscaleState, site::vpn::tailscale::Prefs};
use zeroize::Zeroize;

use super::Op;

/// What the agent does. Every action but [`Logout`](Self::Logout) and [`Down`](Self::Down)
/// gives the CLI [`Tailscale::prefs`], with the tailnet's DNS and routes refused and the
/// firewall left to fw4 (`--netfilter-mode=off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleAction {
    /// `tailscale set`: the prefs, logged in or not.
    Set,
    /// `tailscale up --reset`: running again after `down`.
    Up,
    /// `tailscale down`: stopped, still logged in.
    Down,
    /// `tailscale logout`: the prefs stay.
    Logout,
    /// `tailscale login`: with [`Tailscale::auth_key`], by itself; without, in the background,
    /// answering the login URL `tailscaled` hands out (or the one already waiting). A machine
    /// logged in gets no URL.
    Login,
}

/// [`code::NOT_FOUND`](super::code::NOT_FOUND) without the package; [`code::FAILED`](super::code::FAILED)
/// with the CLI's words, such as `backend error: invalid key: unable to validate API key`.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Tailscale {
    pub action: TailscaleAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefs: Option<Prefs>,
    /// For a login by itself: the key goes to the CLI as a file of root's, removed after.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_key: Option<String>,
}

impl Tailscale {
    /// `action` with `prefs` (ignored by `down` and `logout`).
    pub fn new(action: TailscaleAction, prefs: Prefs) -> Self {
        Self {
            action,
            prefs: (!matches!(action, TailscaleAction::Down | TailscaleAction::Logout))
                .then_some(prefs),
            auth_key: None,
        }
    }

    /// A logout: the prefs stay.
    pub const fn logout() -> Self {
        Self {
            action: TailscaleAction::Logout,
            prefs: None,
            auth_key: None,
        }
    }

    /// A login by itself with `key`.
    pub fn login_with(prefs: Prefs, key: String) -> Self {
        Self {
            action: TailscaleAction::Login,
            prefs: Some(prefs),
            auth_key: Some(key),
        }
    }
}

impl Drop for Tailscale {
    fn drop(&mut self) {
        self.auth_key.zeroize();
    }
}

impl fmt::Debug for Tailscale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tailscale")
            .field("action", &self.action)
            .field("prefs", &self.prefs)
            .field("auth_key", &self.auth_key.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Op for Tailscale {
    const NAME: &'static str = "tailscale";
    type Output = TailscaleAnswer;
}

/// The daemon afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TailscaleAnswer {
    /// Its state, as `status --json` writes it (`NeedsLogin`, `Running`, …).
    #[serde(deserialize_with = "state")]
    pub state: TailscaleState,
    /// For a login without a key: the URL that logs it in.
    #[serde(default)]
    pub auth_url: Option<String>,
}

fn state<'de, D: serde::Deserializer<'de>>(d: D) -> Result<TailscaleState, D::Error> {
    let text = String::deserialize(d)?;
    Ok(TailscaleState::from_backend(&text))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use uniwrt_model::{facts::TailscaleState, site::vpn::tailscale::Prefs};

    use super::{Tailscale, TailscaleAction, TailscaleAnswer};

    fn prefs() -> Prefs {
        Prefs {
            routes: vec!["10.0.0.0/24".into()],
            exit_node: true,
            hostname: String::new(),
        }
    }

    #[test]
    fn requests_carry_the_prefs_where_the_cli_takes_them_and_hide_the_key() {
        let set = serde_json::to_value(Tailscale::new(TailscaleAction::Set, prefs())).unwrap();
        assert_eq!(
            set,
            serde_json::json!({"action": "set", "prefs": {
                "routes": ["10.0.0.0/24"], "exit_node": true, "hostname": ""
            }})
        );
        let down = serde_json::to_value(Tailscale::new(TailscaleAction::Down, prefs())).unwrap();
        assert_eq!(down, serde_json::json!({"action": "down"}));
        let login = Tailscale::login_with(prefs(), "tskey-auth-secret".into());
        assert_eq!(
            serde_json::to_value(&login).unwrap()["auth_key"],
            "tskey-auth-secret"
        );
        assert!(!format!("{login:?}").contains("secret"));
    }

    #[test]
    fn answers_carry_the_backend_state_and_the_url() {
        let answer: TailscaleAnswer = serde_json::from_value(serde_json::json!({
            "state": "NeedsLogin", "auth_url": "https://login.tailscale.com/a/abc"
        }))
        .unwrap();
        assert_eq!(answer.state, TailscaleState::NeedsLogin);
        assert_eq!(
            answer.auth_url.as_deref(),
            Some("https://login.tailscale.com/a/abc")
        );
        let answer: TailscaleAnswer =
            serde_json::from_value(serde_json::json!({"state": "Running", "auth_url": null}))
                .unwrap();
        assert_eq!(
            (answer.state, answer.auth_url),
            (TailscaleState::Running, None)
        );
    }
}
