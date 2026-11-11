//! Client bans (features/wifi.md, Client actions; D-084): the controller keeps blocked clients
//! off every managed access point, and kicked ones for as long as the kick said, through each
//! node's agent (the `ban` op: hostapd's `del_client` with a ban time on every BSS). Nothing is
//! configured on the nodes: hostapd keeps bans in memory, for at most [`BansConfig::chunk`] at a
//! time, so the controller renews them while it runs; while it is down they lapse within that.
//!
//! The clients' book says who is blocked (users block and unblock through
//! [`Engine::block_client`], which the store keeps); kicks live here, in memory. The saved site
//! (its head revision) says which nodes are managed: those that manage `wireless`. Every
//! `tick_every`, a managed node that is online and not held by a job gets every wanted ban when
//! one is missing or within `renew_before` of lapsing, and a lift for each ban no longer
//! wanted ([`plan`]). What it holds is forgotten when it goes offline, so it gets all again when
//! back; a job's end (an apply's `wifi reload` restarts BSSes) and a wanted client seen
//! connected to it send all again at once (at most every `settle`).
//!
//! [`Bans::kick`] deauthenticates a client on every managed node, banning it for a while. A
//! node that refuses raises `node:<id>/client-bans` ([`EventKind::ClientBansFailed`]), cleared
//! once it takes them ([`EventKind::ClientBansOk`]); kicks are announced as
//! [`EventKind::ClientKicked`].
//!
//! [`EventKind::ClientBansFailed`]: uniwrt_model::events::EventKind::ClientBansFailed
//! [`EventKind::ClientBansOk`]: uniwrt_model::events::EventKind::ClientBansOk
//! [`EventKind::ClientKicked`]: uniwrt_model::events::EventKind::ClientKicked
//! [`Engine::block_client`]: crate::Engine::block_client

mod plan;
mod task;

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, mpsc, oneshot},
    task::JoinHandle,
};
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::job::Job;
use uniwrt_node::NodeLink;
use uniwrt_store::RevisionRepo;

use crate::Engine;

#[derive(Debug, Clone)]
pub struct BansConfig {
    pub tick_every: Duration,
    /// The longest ban sent at once (hostapd takes at most 30 minutes).
    pub chunk: Duration,
    /// How long before a node's ban lapses it is renewed.
    pub renew_before: Duration,
    /// How long after sending a node its bans they aren't forced on it again.
    pub settle: Duration,
    /// How long after a refusal the same node is asked again.
    pub retry: Duration,
    /// How long the `ban` op may take.
    pub timeout: Duration,
}

impl Default for BansConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(30),
            chunk: Duration::from_mins(10),
            renew_before: Duration::from_mins(5),
            settle: Duration::from_secs(10),
            retry: Duration::from_mins(1),
            timeout: Duration::from_secs(15),
        }
    }
}

/// How a kick went: the access points the client was deauthenticated from, and the managed
/// nodes that couldn't take it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kicked {
    /// The managed nodes asked.
    pub nodes: u32,
    pub from: Vec<KickedFrom>,
    pub failed: Vec<KickFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KickedFrom {
    pub node: NodeId,
    pub ifname: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KickFailure {
    pub node: NodeId,
    pub error: String,
}

/// Why a kick couldn't be asked for.
#[derive(Debug, thiserror::Error)]
pub enum BansError {
    #[error("client bans are not running")]
    Stopped,
    #[error("a kick bans for at most {max:?}")]
    TooLong { max: Duration },
}

/// Keeps the bans until stopped.
pub struct ClientBans {
    stop: oneshot::Sender<()>,
    commands: mpsc::Sender<task::Command>,
    task: JoinHandle<()>,
}

/// The API's way to the bans. Clones share it.
#[derive(Clone)]
pub struct Bans {
    commands: mpsc::Sender<task::Command>,
}

impl ClientBans {
    /// Starts keeping them; `jobs` tells which nodes a job holds.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: BansConfig,
    ) -> Self
    where
        L: NodeLink,
        R: RevisionRepo + Clone + 'static,
    {
        let bus = engine.subscribe();
        let (stop, stopped) = oneshot::channel();
        let (commands, received) = mpsc::channel(16);
        let task = task::Task::new(engine.clone(), repo, config);
        let task = tokio::spawn(task.run(bus, jobs, received, stopped));
        Self {
            stop,
            commands,
            task,
        }
    }

    pub fn handle(&self) -> Bans {
        Bans {
            commands: self.commands.clone(),
        }
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

impl Bans {
    /// The longest kick.
    pub const MAX_KICK: Duration = Duration::from_hours(24);

    /// Deauthenticates `mac` on every managed node and keeps it off them for `ban` (none: it
    /// may come back at once); `by` names the user, for the event.
    pub async fn kick(
        &self,
        mac: MacAddr,
        ban: Duration,
        by: Option<String>,
    ) -> Result<Kicked, BansError> {
        if ban > Self::MAX_KICK {
            return Err(BansError::TooLong {
                max: Self::MAX_KICK,
            });
        }
        let (reply, answer) = oneshot::channel();
        let command = task::Command::Kick {
            mac,
            ban,
            by,
            reply,
        };
        self.commands
            .send(command)
            .await
            .map_err(|_| BansError::Stopped)?;
        answer.await.map_err(|_| BansError::Stopped)
    }
}

#[cfg(test)]
mod tests;
