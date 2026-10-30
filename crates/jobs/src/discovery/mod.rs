//! Discovery (features/adoption.md, Discovery): the devices the adopted nodes see
//! ([`sightings`]), probed on port 22, and reported when they look like OpenWrt nodes: a dropbear
//! banner, a default OpenWrt or uniwrt hostname, or a batman-adv originator that nothing else
//! accounts for. A probe reads the SSH banner and, for those, the host key; it never logs in.
//! Probes run at most [`PROBES`] at a time, and each result holds for [`FRESH`].

mod sightings;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use tokio::{
    sync::{Semaphore, watch},
    time::{Instant, timeout_at},
};
use tracing::debug;
use uniwrt_core::{Clock, MacAddr, Timestamp};
use uniwrt_model::{
    discovery::{Discovered, DiscoveryReport, DiscoverySource, PresentedKey, ProbeState},
    facts::NodeFacts,
    node::Node,
};
use uniwrt_node::{HostKey, Target};

use self::sightings::{Sighting, sightings};
use crate::Dial;

/// Probes running at once.
const PROBES: usize = 4;
/// How long a probe's result holds; a probe still running after this counts as lost.
const FRESH: Duration = Duration::from_secs(600);
const SSH_PORT: u16 = 22;

/// A device's probe, by MAC and IP address.
type Key = (MacAddr, IpAddr);

pub(crate) struct Discovery {
    dial: Option<Arc<dyn Dial>>,
    clock: Arc<dyn Clock>,
    probes: Arc<Mutex<BTreeMap<Key, Probe>>>,
    permits: Arc<Semaphore>,
    /// Bumped as each probe ends.
    ended: watch::Sender<()>,
}

#[derive(Clone)]
enum Probe {
    Running { since: Timestamp },
    Done(Arc<Probed>),
}

struct Probed {
    at: Timestamp,
    /// Port 22 accepted the connection.
    reached: bool,
    ssh: Option<String>,
    host_key: Option<HostKey>,
}

impl Discovery {
    /// Without a dial, nothing is probed.
    pub(crate) fn new(dial: Option<Arc<dyn Dial>>, clock: Arc<dyn Clock>) -> Self {
        Self {
            dial,
            clock,
            probes: Arc::default(),
            permits: Arc::new(Semaphore::new(PROBES)),
            ended: watch::channel(()).0,
        }
    }

    /// Probes what the adopted nodes see, unless probed lately, waits up to `wait` for the
    /// probes, and reports.
    pub(crate) async fn scan(
        &self,
        adopted: &[(Node, Option<Arc<NodeFacts>>)],
        wait: Duration,
    ) -> DiscoveryReport {
        let sightings = sightings(adopted);
        let mut ended = self.ended.subscribe();
        self.start(&sightings);
        let deadline = Instant::now() + wait;
        while self.running(&sightings) {
            if timeout_at(deadline, ended.changed()).await.is_err() {
                break;
            }
        }
        self.report(&sightings)
    }

    /// Starts the probes that are due, and forgets those of devices no longer seen.
    fn start(&self, sightings: &[Sighting]) {
        let Some(dial) = &self.dial else {
            return;
        };
        let now = self.clock.now();
        let keys: BTreeSet<Key> = sightings.iter().filter_map(key).collect();
        let mut probes = self.probes();
        probes.retain(|key, _| keys.contains(key));
        for sighting in sightings {
            let Some(key) = key(sighting) else {
                continue;
            };
            let since = match probes.get(&key) {
                None => None,
                Some(Probe::Running { since }) => Some(*since),
                Some(Probe::Done(done)) => Some(done.at),
            };
            if since.is_some_and(|since| now.saturating_duration_since(since) < FRESH) {
                continue;
            }
            probes.insert(key, Probe::Running { since: now });
            let task = ProbeTask {
                dial: Arc::clone(dial),
                clock: Arc::clone(&self.clock),
                probes: Arc::clone(&self.probes),
                permits: Arc::clone(&self.permits),
                ended: self.ended.clone(),
                key,
                wants_key: sighting.openwrt_name(),
            };
            tokio::spawn(task.run());
        }
    }

    fn running(&self, sightings: &[Sighting]) -> bool {
        let probes = self.probes();
        sightings
            .iter()
            .filter_map(key)
            .any(|key| matches!(probes.get(&key), Some(Probe::Running { .. })))
    }

    fn report(&self, sightings: &[Sighting]) -> DiscoveryReport {
        let probes = self.probes().clone();
        let probe = |sighting: &Sighting| key(sighting).and_then(|key| probes.get(&key).cloned());
        let mut nodes = Vec::new();
        let mut scanning = false;
        // Each originator: who sees it, whether a probe of its devices runs, and whether one of
        // them is reported.
        let mut meshes: BTreeMap<MacAddr, (&Sighting, bool, bool)> = BTreeMap::new();
        for sighting in sightings {
            let probe = probe(sighting);
            let probing = matches!(probe, Some(Probe::Running { .. }));
            scanning |= probing;
            let node = discovered(sighting, probe.as_ref(), self.dial.is_some());
            let shown = sighting.openwrt_name() || node.ssh.as_deref().is_some_and(dropbear);
            if let Some(originator) = sighting.originator {
                let mesh = meshes.entry(originator).or_insert((sighting, false, false));
                mesh.1 |= probing;
                mesh.2 |= shown;
            }
            if shown {
                nodes.push(node);
            }
        }
        // A mesh node none of whose devices answered as one: known only by its originator.
        for (originator, (sighting, probing, shown)) in meshes {
            if !shown {
                nodes.push(Discovered {
                    mac: originator,
                    ip: None,
                    hostname: None,
                    sources: vec![DiscoverySource::Mesh],
                    seen_by: sighting.seen_by,
                    probe: if probing {
                        ProbeState::Probing
                    } else {
                        ProbeState::Skipped
                    },
                    ssh: None,
                    host_key: None,
                    probed_at: None,
                });
            }
        }
        nodes.sort_by_key(|node| (node.ip.is_none(), node.ip, node.mac));
        DiscoveryReport { nodes, scanning }
    }

    fn probes(&self) -> MutexGuard<'_, BTreeMap<Key, Probe>> {
        lock(&self.probes)
    }
}

fn key(sighting: &Sighting) -> Option<Key> {
    sighting.ip.map(|ip| (sighting.mac, ip))
}

fn lock(probes: &Mutex<BTreeMap<Key, Probe>>) -> MutexGuard<'_, BTreeMap<Key, Probe>> {
    probes.lock().unwrap_or_else(PoisonError::into_inner)
}

/// dropbear is OpenWrt's SSH server.
fn dropbear(ssh: &str) -> bool {
    ssh.to_ascii_lowercase().contains("dropbear")
}

fn discovered(sighting: &Sighting, probe: Option<&Probe>, can_probe: bool) -> Discovered {
    let mut node = Discovered {
        mac: sighting.mac,
        ip: sighting.ip,
        hostname: sighting.hostname.clone(),
        sources: sighting.sources.iter().copied().collect(),
        seen_by: sighting.seen_by,
        probe: ProbeState::Skipped,
        ssh: None,
        host_key: None,
        probed_at: None,
    };
    match probe {
        _ if !can_probe || sighting.ip.is_none() => {}
        None | Some(Probe::Running { .. }) => node.probe = ProbeState::Probing,
        Some(Probe::Done(probed)) => {
            node.probe = if probed.reached {
                ProbeState::Done
            } else {
                ProbeState::Unreachable
            };
            node.ssh.clone_from(&probed.ssh);
            node.host_key = probed.host_key.as_ref().map(|key| PresentedKey {
                host_key: key.to_openssh().to_owned(),
                fingerprint: key.fingerprint().to_owned(),
                algorithm: key.algorithm(),
            });
            node.probed_at = Some(probed.at);
        }
    }
    node
}

/// One probe, in its own task.
struct ProbeTask {
    dial: Arc<dyn Dial>,
    clock: Arc<dyn Clock>,
    probes: Arc<Mutex<BTreeMap<Key, Probe>>>,
    permits: Arc<Semaphore>,
    ended: watch::Sender<()>,
    key: Key,
    /// Read the host key even if the server is not dropbear.
    wants_key: bool,
}

impl ProbeTask {
    async fn run(self) {
        let (mac, ip) = self.key;
        let target = Target::new(ip.to_string(), SSH_PORT);
        let probed = {
            // Never closed.
            let _permit = self.permits.acquire().await;
            let banner = self.dial.banner(&target).await;
            let ssh = banner
                .as_ref()
                .inspect_err(|err| debug!(%mac, %ip, %err, "no SSH"))
                .ok()
                .cloned()
                .flatten();
            let host_key = match &ssh {
                Some(ssh) if self.wants_key || dropbear(ssh) => self
                    .dial
                    .probe(&target)
                    .await
                    .inspect_err(|err| debug!(%mac, %ip, %err, "no host key"))
                    .ok(),
                _ => None,
            };
            Probed {
                at: self.clock.now(),
                reached: banner.is_ok(),
                ssh,
                host_key,
            }
        };
        lock(&self.probes).insert(self.key, Probe::Done(Arc::new(probed)));
        self.ended.send_replace(());
    }
}
