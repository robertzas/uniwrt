//! Tailscale on the gateway (features/network.md, VPN; D-140): the controller keeps the gateway's
//! `tailscaled` to the site, through the agent's `tailscale` op. Its prefs (routes, exit node,
//! name; the tailnet's DNS and routes refused; the firewall left to fw4) live in the daemon's
//! state file, not UCI, so no apply carries them: render brings the package and the zone, and
//! this task the rest.
//!
//! The saved site (its head revision) says what is wanted; the gateway's `services` facts
//! (`tailscale status --json` and `tailscale debug prefs`, every minute) say what is. Every
//! `tick_every`, and whenever the gateway's `services` set is collected, the managed gateway
//! is compared ([`step`]): prefs that differ get `set`; a stopped daemon the site wants on
//! gets `up`, and one it wants off `down`; a logged-out daemon with an auth key in the
//! vault logs in with it. Without a key a login waits for an admin (the API's login URL). A
//! site without `vpn.tailscale` leaves the daemon alone, as do an observed gateway and one a
//! job holds. After acting it waits `settle` for the facts to catch up, and `retry` before
//! sending the same again. A refusal (a bad key, say) raises `tailscale_failed`
//! (`node:<id>/tailscale`), cleared by `tailscale_ok` once the gateway takes a request.

mod task;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, oneshot},
    task::JoinHandle,
};
use uniwrt_model::{
    facts::{TailscaleFacts, TailscaleState},
    job::Job,
    site::{SiteConfig, vpn::tailscale::Prefs},
};
use uniwrt_node::{NodeLink, proto::ops::TailscaleAction};
use uniwrt_store::{RevisionRepo, VaultRepo};

use crate::Engine;

/// How long a `tailscale` request may take: a login waits up to 30 s for the daemon to run.
pub const TAILSCALE_TIMEOUT: Duration = Duration::from_mins(1);

#[derive(Debug, Clone)]
pub struct TailscaleConfig {
    pub tick_every: Duration,
    /// How long after acting the gateway's facts may still show the daemon as it was (the
    /// `services` set comes every minute).
    pub settle: Duration,
    /// How long before the same request goes to the gateway again.
    pub retry: Duration,
}

impl Default for TailscaleConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(30),
            settle: Duration::from_secs(90),
            retry: Duration::from_mins(10),
        }
    }
}

/// What the gateway's daemon needs, if anything, with the prefs to send: `site`'s Tailscale
/// against the daemon's `facts`. A login is wanted only with an auth key.
pub fn step(site: &SiteConfig, facts: &TailscaleFacts) -> Option<(TailscaleAction, Prefs)> {
    let tailscale = site.vpn.tailscale.as_ref()?;
    let (Some(state), Some(held)) = (facts.state, facts.prefs.as_ref()) else {
        return None;
    };
    let prefs = tailscale.prefs(site);
    let action = if !tailscale.enabled {
        held.want_running.then_some(TailscaleAction::Down)?
    } else if !prefs.held_by(held) {
        TailscaleAction::Set
    } else if state == TailscaleState::NeedsLogin {
        tailscale.auth_key.as_ref()?;
        TailscaleAction::Login
    } else if !held.want_running && !held.logged_out {
        TailscaleAction::Up
    } else {
        return None;
    };
    Some((action, prefs))
}

/// Keeps the gateway's Tailscale to the site until stopped.
pub struct TailscaleSync {
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl TailscaleSync {
    /// Starts watching; `vault` holds the auth key (without it, no login by key), `jobs` tells
    /// which nodes a job holds.
    pub fn start<L, R, V>(
        engine: &Engine<L>,
        repo: R,
        vault: Option<V>,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: TailscaleConfig,
    ) -> Self
    where
        L: NodeLink,
        R: RevisionRepo + Clone + 'static,
        V: VaultRepo + Clone + 'static,
    {
        let bus = engine.subscribe();
        let (stop, stopped) = oneshot::channel();
        let task = task::Task::new(engine.clone(), repo, vault, config);
        let task = tokio::spawn(task.run(bus, jobs, stopped));
        Self { stop, task }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}
