//! DNS filtering's blocklists on the gateway (features/network.md, Blocklists; D-133). The
//! lists' copies are the `dns` crate's ([`uniwrt_dns::Blocklists`]); this service decides when
//! they refresh, compiles them for the gateway and keeps the gateway's copy current.
//!
//! The gateway is the watched node whose role is Gateway, and its **applied** revision says what
//! it gets: nothing unless that revision manages its `dhcp` (the files of a gateway the
//! controller doesn't manage are left alone); with `services.blocking` on, that revision's
//! lists, allow and deny compiled into at most what its memory holds
//! ([`capacity`](uniwrt_model::blocklists::capacity) of `MemTotal`); with it off, the files are
//! removed once its `services` set reports them.
//!
//! **Refresh:** the lists of the saved (head) and applied revisions where blocking is on, so a
//! saved choice shows its lists before it is applied: at once while one of them was never asked
//! for, in the site's nightly window (04:00–06:00 local) once the least recently asked for is
//! over [`BlocklistConfig::stale`], at any time once it is over [`BlocklistConfig::overdue`],
//! and on demand ([`Blocklist::refresh`]). A compile follows when the applied settings, a copy
//! or the gateway's memory changed.
//!
//! **Push:** while the gateway is online and no job holds it, when the SHA-256 its `services`
//! set reports differs from the compiled file's (or the conf file is missing): `mkdir -p` the
//! conf directory and upload the list, then signal dnsmasq (`SIGHUP` through procd: it reads the
//! servers-file again; a restart when procd finds no dnsmasq running) when the conf file is
//! there, else upload the conf file and restart dnsmasq once. The same file isn't pushed again within [`BlocklistConfig::repush`] (the set
//! reports the new one within a minute), and a failed push is tried again after it. The
//! compiled text is kept only until it is pushed (a later push, after a reboot emptied the
//! gateway's `/tmp`, compiles it again), so the controller holds no large buffer.
//!
//! [`Blocklist::state`] says where all of it stands ([`BlocklistState`]).

mod push;
mod task;

use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    task::JoinHandle,
};
use uniwrt_dns::Blocklists;
use uniwrt_model::{blocklists::BlocklistState, job::Job};
use uniwrt_node::NodeLink;
use uniwrt_store::{ApplyRepo, RevisionRepo};

use crate::Engine;

#[derive(Debug, Clone)]
pub struct BlocklistConfig {
    /// How often the gateway's state and the revisions are read again.
    pub tick_every: Duration,
    /// The nightly window: minutes after local midnight, and its length.
    pub window_at_min: u16,
    pub window_len_min: u16,
    /// How old the least recently asked for list is before the window refreshes the lists.
    pub stale: Duration,
    /// How old it is before they are refreshed at any time.
    pub overdue: Duration,
    /// How long after a push the same file isn't pushed again.
    pub repush: Duration,
    /// How long each command or upload of a push may take.
    pub timeout: Duration,
}

impl Default for BlocklistConfig {
    fn default() -> Self {
        Self {
            tick_every: Duration::from_secs(30),
            window_at_min: 4 * 60,
            window_len_min: 2 * 60,
            stale: Duration::from_hours(20),
            overdue: Duration::from_hours(48),
            repush: Duration::from_mins(5),
            timeout: Duration::from_mins(2),
        }
    }
}

/// Why a refresh couldn't be asked for.
#[derive(Debug, thiserror::Error)]
pub enum BlocklistError {
    #[error("DNS filtering is not running")]
    Stopped,
    #[error("DNS filtering is off: no list to refresh")]
    Off,
}

/// Keeps the gateway's blocklist until stopped.
pub struct BlocklistSync {
    stop: oneshot::Sender<()>,
    handle: Blocklist,
    task: JoinHandle<()>,
}

/// The API's way to the blocklists. Clones share it.
#[derive(Clone)]
pub struct Blocklist {
    commands: mpsc::Sender<task::Command>,
    state: watch::Receiver<Arc<BlocklistState>>,
}

impl BlocklistSync {
    /// Starts keeping it, with the copies in `lists`; `jobs` tells which nodes a job holds.
    pub fn start<L, R>(
        engine: &Engine<L>,
        repo: R,
        lists: Blocklists,
        jobs: Option<broadcast::Receiver<Arc<Job>>>,
        config: BlocklistConfig,
    ) -> Self
    where
        L: NodeLink,
        R: RevisionRepo + ApplyRepo + Clone + 'static,
    {
        let bus = engine.subscribe();
        let (stop, stopped) = oneshot::channel();
        let (commands, received) = mpsc::channel(8);
        let (state, watched) = watch::channel(Arc::new(task::empty()));
        let task = task::Task::new(engine.clone(), repo, lists, config, state);
        let task = tokio::spawn(task.run(bus, jobs, received, stopped));
        Self {
            stop,
            handle: Blocklist {
                commands,
                state: watched,
            },
            task,
        }
    }

    pub fn handle(&self) -> Blocklist {
        self.handle.clone()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.task.await;
    }
}

impl Blocklist {
    /// Where the lists, the compiled file and the gateway's copy stand.
    pub fn state(&self) -> Arc<BlocklistState> {
        Arc::clone(&self.state.borrow())
    }

    /// Downloads the lists again now (conditionally); returns once it started, or was already
    /// running. [`BlocklistError::Off`] while no revision has blocking on.
    pub async fn refresh(&self) -> Result<(), BlocklistError> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(task::Command::Refresh { reply })
            .await
            .map_err(|_| BlocklistError::Stopped)?;
        answer.await.map_err(|_| BlocklistError::Stopped)?
    }
}

#[cfg(test)]
mod tests;
