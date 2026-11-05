//! What users do about drift: acknowledge one, mute a package or watch it again.

use uniwrt_core::NodeId;
use uniwrt_model::drift::{Acknowledged, DriftChange, DriftEnd, DriftMute};
use uniwrt_node::NodeLink;
use uniwrt_store::DriftRepo;

use super::Task;
use crate::drift::DriftError;

impl<L: NodeLink, R: DriftRepo + Clone + 'static> Task<L, R> {
    pub(super) async fn acknowledge(
        &mut self,
        node: NodeId,
        package: String,
        by: Option<String>,
    ) -> Result<(), DriftError> {
        let key = (node, package);
        let at = self.now();
        let Some(record) = self.records.get(&key) else {
            return Err(DriftError::NotFound {
                node,
                package: key.1,
            });
        };
        if record.acknowledged.is_some() {
            return Ok(());
        }
        let mut record = record.clone();
        record.acknowledged = Some(Acknowledged { at, by });
        self.flush().await;
        self.repo.put_drift(record.clone()).await?;
        self.records.insert(key.clone(), record);
        self.refresh_view(&key);
        self.changes.push(DriftChange::Ended {
            node,
            package: key.1,
            how: DriftEnd::Acknowledged,
        });
        Ok(())
    }

    pub(super) async fn mute(
        &mut self,
        node: NodeId,
        package: String,
        muted: bool,
        by: Option<String>,
    ) -> Result<(), DriftError> {
        let key = (node, package);
        self.repo
            .set_drift_mute(node, key.1.clone(), muted, by.clone())
            .await?;
        if muted {
            if !self.mutes.contains_key(&key) {
                let at = self.now();
                self.mutes.insert(
                    key.clone(),
                    DriftMute {
                        node,
                        package: key.1.clone(),
                        at,
                        by,
                    },
                );
            }
            self.end(&key, DriftEnd::Muted);
        } else {
            self.mutes.remove(&key);
            self.look(node);
        }
        Ok(())
    }
}
