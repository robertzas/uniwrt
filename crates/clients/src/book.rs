//! [`Book`]: every client the controller knows, online or not: updated from each topology
//! build, restored from the store at start, renamed by users, and pruned once long gone. Each
//! call returns its [`Changes`]: the views that changed, and the records, sessions and samples
//! to keep.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{
    client::{ClientRecord, ClientSession, ClientWifi},
    facts::StationCapability,
};
use uniwrt_topo::{Member, Topology};

use crate::{Client, ClientSample, entry::Entry, view::observe};

#[derive(Debug, Clone, Copy)]
pub struct BookConfig {
    /// The least time between two samples of a client.
    pub sample_every: Duration,
    /// How far back the samples go.
    pub samples_kept: Duration,
    /// How long after leaving one place a client showing up at another has roamed.
    pub roam_window: Duration,
    /// How often an online client's `last_seen` is saved.
    pub save_every: Duration,
    /// How long a wired client may go unseen and keep its session.
    pub wired_gap: Duration,
    /// How long a client without an alias is kept after it was last seen.
    pub forget_after: Duration,
}

impl Default for BookConfig {
    fn default() -> Self {
        Self {
            sample_every: Duration::from_secs(5),
            samples_kept: Duration::from_hours(1),
            roam_window: Duration::from_secs(15),
            save_every: Duration::from_mins(5),
            wired_gap: Duration::from_mins(5),
            forget_after: Duration::from_hours(30 * 24),
        }
    }
}

/// What a call changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    /// Views that changed, as they are now.
    pub clients: Vec<Arc<Client>>,
    /// Clients forgotten.
    pub removed: Vec<MacAddr>,
    /// Sessions opened, ended or changed: to save (by MAC and start).
    pub sessions: Vec<ClientSession>,
    /// Records to save (their alias aside, which only [`Book::rename`] changes).
    pub records: Vec<ClientRecord>,
    pub samples: Vec<(MacAddr, ClientSample)>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
            && self.removed.is_empty()
            && self.sessions.is_empty()
            && self.records.is_empty()
            && self.samples.is_empty()
    }
}

/// One client now, for its page: its view, recent samples (oldest first) and open session.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub client: Arc<Client>,
    pub samples: Vec<ClientSample>,
    pub session: Option<ClientSession>,
}

#[derive(Debug, Default)]
pub struct Book {
    config: BookConfig,
    entries: BTreeMap<MacAddr, Entry>,
    /// Each group's alias, as the views were last made with.
    aliases: BTreeMap<String, String>,
}

impl Book {
    pub fn new(config: BookConfig) -> Self {
        Self {
            config,
            entries: BTreeMap::new(),
            aliases: BTreeMap::new(),
        }
    }

    /// Takes in what the controller stored: the records and each client's latest session. A
    /// session still open is closed when the client is not seen at the next build, or goes on
    /// if it is still associated.
    pub fn restore(&mut self, records: Vec<ClientRecord>, sessions: Vec<ClientSession>) -> Changes {
        let mut touched = BTreeSet::new();
        for record in records {
            touched.insert(record.mac);
            match self.entries.get_mut(&record.mac) {
                Some(entry) => {
                    entry.record.alias = record.alias;
                    entry.record.blocked = record.blocked;
                    entry.record.first_seen = entry.record.first_seen.min(record.first_seen);
                    let ClientWifi {
                        standard,
                        bands,
                        capabilities,
                    } = record.wifi;
                    let wifi = &mut entry.record.wifi;
                    wifi.standard = wifi.standard.max(standard);
                    wifi.bands.extend(bands);
                    wifi.capabilities.extend(capabilities);
                }
                None => {
                    self.entries.insert(record.mac, Entry::new(record));
                }
            }
        }
        for session in sessions {
            let Some(entry) = self.entries.get_mut(&session.mac) else {
                continue;
            };
            entry.restore_place(&session);
            if session.end.is_none() {
                if entry.session.is_none() && !entry.online {
                    entry.session = Some(session);
                }
            } else if entry.last.is_none() && entry.session.is_none() {
                entry.last = Some(session);
            }
        }
        let mut changes = Changes::default();
        self.refresh(&touched, &mut changes);
        changes
    }

    /// Takes in a topology build and the members it was built from.
    pub fn update(&mut self, topology: &Topology, members: &[Member<'_>]) -> Changes {
        let seen = observe(topology, members);
        let mut changes = Changes::default();
        let mut touched = BTreeSet::new();
        for (mac, seen) in &seen {
            let entry = self.entries.entry(*mac).or_insert_with(|| {
                let record = ClientRecord {
                    mac: *mac,
                    alias: None,
                    hostname: None,
                    first_seen: seen.at,
                    last_seen: seen.at,
                    blocked: None,
                    wifi: ClientWifi::default(),
                };
                changes.records.push(record.clone());
                Entry::new(record)
            });
            if seen.online {
                entry.online(seen, &self.config, &mut changes);
            } else {
                entry.offline(&mut changes);
            }
            touched.insert(*mac);
        }
        for (mac, entry) in &mut self.entries {
            if !seen.contains_key(mac) && (entry.online || entry.session.is_some()) {
                entry.offline(&mut changes);
                touched.insert(*mac);
            }
        }
        self.refresh(&touched, &mut changes);
        changes
    }

    /// Gives a client an alias, or takes it away (`None`); `None` for an unknown client. The
    /// changes carry the views of its group, which may share the alias.
    pub fn rename(
        &mut self,
        mac: MacAddr,
        alias: Option<String>,
    ) -> Option<(ClientRecord, Changes)> {
        let entry = self.entries.get_mut(&mac)?;
        entry.record.alias = alias;
        let record = entry.record.clone();
        let mut changes = Changes::default();
        self.refresh(&BTreeSet::from([mac]), &mut changes);
        Some((record, changes))
    }

    /// Blocks a client from `at`, or unblocks it (`None`); `None` for an unknown client. A
    /// block is the address's alone: a randomized address's group doesn't share it.
    pub fn block(
        &mut self,
        mac: MacAddr,
        at: Option<Timestamp>,
    ) -> Option<(ClientRecord, Changes)> {
        let entry = self.entries.get_mut(&mac)?;
        entry.record.blocked = at;
        let record = entry.record.clone();
        let mut changes = Changes::default();
        self.refresh(&BTreeSet::from([mac]), &mut changes);
        Some((record, changes))
    }

    /// Notes a capability a client showed outside the stations' facts, such as 802.11r when it
    /// fast-roamed; `None` when the client is unknown or the capability known already.
    pub fn learn(&mut self, mac: MacAddr, capability: StationCapability) -> Option<Changes> {
        let entry = self.entries.get_mut(&mac)?;
        if !entry.record.wifi.add(capability) {
            return None;
        }
        let mut changes = Changes::default();
        entry.save(&mut changes);
        self.refresh(&BTreeSet::from([mac]), &mut changes);
        Some(changes)
    }

    /// The blocked clients' addresses.
    pub fn blocked(&self) -> Vec<MacAddr> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.record.blocked.is_some())
            .map(|(mac, _)| *mac)
            .collect()
    }

    /// Forgets the clients neither named nor blocked last seen before `now` less
    /// [`BookConfig::forget_after`].
    pub fn prune(&mut self, now: Timestamp) -> Changes {
        let before = now.saturating_sub(self.config.forget_after);
        let mut changes = Changes::default();
        self.entries.retain(|mac, entry| {
            let keep = entry.online
                || entry.session.is_some()
                || entry.record.alias.is_some()
                || entry.record.blocked.is_some()
                || entry.record.last_seen >= before;
            if !keep {
                changes.removed.push(*mac);
            }
            keep
        });
        changes
    }

    /// The records whose `last_seen` was not saved yet, and the open sessions: what to write
    /// before stopping, or after missing changes.
    pub fn unsaved(&mut self) -> Changes {
        let mut changes = Changes::default();
        for entry in self.entries.values_mut() {
            if entry.record.last_seen > entry.saved {
                entry.save(&mut changes);
            }
            changes.sessions.extend(entry.session.iter().cloned());
            changes.sessions.extend(entry.last.iter().cloned());
        }
        changes
    }

    /// Every client, by MAC address.
    pub fn clients(&self) -> Vec<Arc<Client>> {
        self.entries
            .values()
            .map(|entry| Arc::clone(&entry.view))
            .collect()
    }

    pub fn detail(&self, mac: MacAddr) -> Option<Detail> {
        let entry = self.entries.get(&mac)?;
        Some(Detail {
            client: Arc::clone(&entry.view),
            samples: entry.samples.iter().copied().collect(),
            session: entry.session.clone(),
        })
    }

    /// Makes the views of the `touched` clients again, and those of every client of a group
    /// whose alias changed; the ones that differ go into `changes`.
    fn refresh(&mut self, touched: &BTreeSet<MacAddr>, changes: &mut Changes) {
        let mut best: BTreeMap<String, (Timestamp, String)> = BTreeMap::new();
        for entry in self.entries.values() {
            if let (Some(group), Some(alias)) = (entry.group(), &entry.record.alias)
                && best
                    .get(&group)
                    .is_none_or(|(seen, _)| entry.record.last_seen > *seen)
            {
                best.insert(group, (entry.record.last_seen, alias.clone()));
            }
        }
        let aliases: BTreeMap<String, String> = best
            .into_iter()
            .map(|(group, (_, alias))| (group, alias))
            .collect();
        let regroup = aliases != self.aliases;
        for (mac, entry) in &mut self.entries {
            if !(touched.contains(mac) || regroup && entry.view.group.is_some()) {
                continue;
            }
            let view = entry.view(Some(&aliases));
            if *entry.view != view {
                entry.view = Arc::new(view);
                changes.clients.push(Arc::clone(&entry.view));
            }
        }
        self.aliases = aliases;
    }
}
