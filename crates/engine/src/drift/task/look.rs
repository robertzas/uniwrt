//! How the watcher looks at a node: its hashes against what was confirmed, the files read
//! back, drifts begun, changed and ended, and what the store confirmed reloaded.

use std::collections::{BTreeMap, BTreeSet};

use tracing::warn;
use uniwrt_core::NodeId;
use uniwrt_model::drift::{Confirmed, DriftChange, DriftEnd, DriftRecord};
use uniwrt_node::NodeLink;
use uniwrt_store::DriftRepo;

use super::{Task, fetch::Reading};
use crate::drift::view::{Key, of, same, sha256, view};

impl<L: NodeLink, R: DriftRepo + Clone + 'static> Task<L, R> {
    /// Compares the node's hashes with what was confirmed: ends the drifts of packages back as
    /// confirmed or no longer watched, and reads the files whose hash is new.
    pub(super) fn look(&mut self, node: NodeId) {
        if self.busy(node) {
            return;
        }
        // An agent without the `config` set reports nothing: /etc/config is never empty.
        let Some(hashes) = self.hashes.get(&node).filter(|h| !h.is_empty()) else {
            return;
        };
        let mut ended = Vec::new();
        let mut reads = Vec::new();
        for (key, _) in of(node, &self.records) {
            if !self.confirmed.contains_key(key) {
                ended.push((key.clone(), DriftEnd::Unwatched));
            } else if self.mutes.contains_key(key) {
                ended.push((key.clone(), DriftEnd::Muted));
            }
        }
        for (key, confirmed) in of(node, &self.confirmed) {
            if self.mutes.contains_key(key) {
                continue;
            }
            let reported = hashes.get(&key.1);
            if reported == Some(&confirmed.sha256) {
                if self.records.contains_key(key) {
                    ended.push((key.clone(), DriftEnd::Reverted));
                }
                continue;
            }
            if self
                .records
                .get(key)
                .is_some_and(|record| record.sha256.as_ref() == reported)
            {
                continue;
            }
            reads.push((key.1.clone(), reported.cloned()));
        }
        for (key, how) in ended {
            self.end(&key, how);
        }
        if !reads.is_empty() && !self.reading.contains(&node) {
            self.read(node, reads);
        }
    }

    pub(super) async fn on_read(&mut self, node: NodeId, readings: Vec<Reading>) {
        self.reading.remove(&node);
        if self.busy(node) {
            return;
        }
        let mut again = false;
        for reading in readings {
            let key = (node, reading.package.clone());
            if self.mutes.contains_key(&key) {
                continue;
            }
            let Some(confirmed) = self.confirmed.get(&key) else {
                continue;
            };
            let now_reported = self.hashes.get(&node).and_then(|h| h.get(&key.1));
            if now_reported != reading.reported.as_ref() {
                // It changed again meanwhile.
                again = true;
                continue;
            }
            let text = match reading.text {
                Ok(text) => text,
                Err(why) => {
                    warn!(%node, package = %key.1, "cannot read the package for drift: {why}");
                    continue;
                }
            };
            // The hash of what was read, not the reported one, which may be stale.
            let sha = text.as_deref().map(sha256);
            if let (Some(text), Some(sha)) = (&text, &sha)
                && same(&key.1, &confirmed.text, text)
            {
                if *sha != confirmed.sha256 {
                    self.reconfirm(&key, sha.clone(), text.clone()).await;
                }
                if self.records.contains_key(&key) {
                    self.end(&key, DriftEnd::Reverted);
                }
                continue;
            }
            if self.records.get(&key).is_some_and(|r| r.sha256 == sha) {
                continue;
            }
            self.drifted(&key, sha, text).await;
        }
        if again {
            self.look(node);
        }
    }

    /// The file changed only in form: what it reads now is what the controller confirmed.
    async fn reconfirm(&mut self, key: &Key, sha256: String, text: String) {
        if let Err(err) = self
            .repo
            .reconfirm(key.0, key.1.clone(), sha256.clone(), text.clone())
            .await
        {
            warn!(node = %key.0, package = %key.1, "cannot store the confirmed package: {err}");
            return;
        }
        if let Some(confirmed) = self.confirmed.get_mut(key) {
            confirmed.sha256 = sha256;
            confirmed.text = text;
        }
    }

    async fn drifted(&mut self, key: &Key, sha256: Option<String>, text: Option<String>) {
        let now = self.now();
        let (detected_at, detected) = match self.records.get(key) {
            Some(old) if old.acknowledged.is_none() => (old.detected_at, false),
            _ => (now, true),
        };
        let record = DriftRecord {
            node: key.0,
            package: key.1.clone(),
            sha256,
            text,
            detected_at,
            changed_at: now,
            acknowledged: None,
        };
        self.flush().await;
        if let Err(err) = self.repo.put_drift(record.clone()).await {
            warn!(node = %key.0, package = %key.1, "cannot store the drift: {err}");
            return;
        }
        let removed = record.text.is_none();
        self.records.insert(key.clone(), record);
        self.refresh_view(key);
        if detected && let Some(confirmed) = self.confirmed.get(key) {
            self.changes.push(DriftChange::Detected {
                node: key.0,
                package: key.1.clone(),
                revision: confirmed.revision,
                removed,
            });
        }
    }

    /// Ends a drift: forgets it and announces how.
    /// Forgets in the store the drifts that ended: before a drift is stored again, and before
    /// the state is announced.
    pub(super) async fn flush(&mut self) {
        for (node, package) in std::mem::take(&mut self.ended) {
            if let Err(err) = self.repo.delete_drift(node, package.clone()).await {
                warn!(%node, %package, "cannot forget the drift: {err}");
            }
        }
    }

    pub(super) fn end(&mut self, key: &Key, how: DriftEnd) {
        let Some(record) = self.records.remove(key) else {
            return;
        };
        self.views.remove(key);
        self.ended.push(key.clone());
        // An acknowledged drift's condition ended with its acknowledgement.
        if record.acknowledged.is_none() {
            self.changes.push(DriftChange::Ended {
                node: key.0,
                package: key.1.clone(),
                how,
            });
        }
    }

    /// What the store holds now, for `node` after a job on it (an apply confirmed its packages
    /// anew, observe mode forgot them), or for every node the tick finds free.
    pub(super) async fn reload(&mut self, node: Option<NodeId>) {
        let confirmed = match self.repo.confirmed(node).await {
            Ok(confirmed) => confirmed,
            Err(err) => {
                warn!("cannot read the confirmed packages: {err}");
                return;
            }
        };
        let mut nodes: BTreeSet<NodeId> = match node {
            Some(node) => BTreeSet::from([node]),
            None => self
                .confirmed
                .keys()
                .chain(self.records.keys())
                .map(|key| key.0)
                .chain(confirmed.iter().map(|c| c.node))
                .collect(),
        };
        nodes.retain(|&node| !self.busy(node));
        let before: BTreeMap<Key, Confirmed> = self
            .confirmed
            .extract_if(.., |key, _| nodes.contains(&key.0))
            .collect();
        for c in confirmed.into_iter().filter(|c| nodes.contains(&c.node)) {
            self.confirmed.insert((c.node, c.package.clone()), c);
        }
        let keys: Vec<Key> = self
            .records
            .keys()
            .filter(|key| nodes.contains(&key.0))
            .cloned()
            .collect();
        for key in keys {
            match (before.get(&key), self.confirmed.get(&key)) {
                (_, None) => self.end(&key, DriftEnd::Unwatched),
                (Some(old), Some(new)) if old.at == new.at && old.revision == new.revision => {
                    // Reconfirmed in another form: the view's diff starts from the new text.
                    self.refresh_view(&key);
                }
                _ => self.end(&key, DriftEnd::Confirmed),
            }
        }
    }

    pub(super) fn forget(&mut self, node: NodeId) {
        let keys: Vec<Key> = of(node, &self.records)
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            self.end(&key, DriftEnd::Unwatched);
        }
        self.confirmed.retain(|key, _| key.0 != node);
        self.mutes.retain(|key, _| key.0 != node);
        self.hashes.remove(&node);
    }

    pub(super) fn refresh_view(&mut self, key: &Key) {
        let (Some(record), Some(confirmed)) = (self.records.get(key), self.confirmed.get(key))
        else {
            self.views.remove(key);
            return;
        };
        self.views.insert(key.clone(), view(confirmed, record));
    }
}
