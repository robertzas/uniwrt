//! Adopting a device into a node's record that exists already (features/lifecycle.md, Replace /
//! RMA / decommission; D-131): a replace names the record, and a node reset to its defaults
//! awaits its device with a pre-approval of its MAC and board, which any adoption path finds
//! here. The record keeps its id, name, role and history; the device's MAC, board, host key and
//! facts replace the old one's, and a revision moves the node's settings to the new device
//! ([`uniwrt_model::site::replace`]): what has no match on it is dropped, and the step says so.

use std::fmt::Write as _;

use tracing::warn;
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{
    job::{JobError, JobErrorCode},
    node::Node,
    site::{
        replace::{Device, Dropped, replace_node},
        revision::RevisionKind,
    },
};
use uniwrt_render::port_names;
use uniwrt_store::{Adoption, FactsRepo as _, NodeRepo as _, ZtpRepo as _};

use super::already_adopted;
use crate::{
    errors::internal,
    revise::{Author, revise},
    runner::Shared,
};

/// Adopting a new device into `node`'s record, in place of its failed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    pub node: NodeId,
    /// Who asked, for the revision moving the node's settings.
    pub by: Option<Author>,
    /// Re-trusting the node's own device after its host key changed (D-154): the device must
    /// be the record's (its MAC and board), and one that still trusts the controller's key
    /// keeps its record as it is ([`super::retrust`]).
    pub retrust: bool,
}

/// The record the device (`mac`, `board`) is adopted into, if not a new one: the one its
/// replace names, else a record with the MAC that awaits it. Another record with the MAC is
/// [`already_adopted`].
pub(super) async fn into(
    shared: &Shared,
    replace: Option<&Replace>,
    mac: MacAddr,
    board: &str,
    adopted: &[Node],
) -> Result<Option<NodeId>, JobError> {
    let found = adopted.iter().find(|node| node.mac == mac);
    if let Some(replace) = replace {
        if let Some(node) = found.filter(|node| node.id != replace.node) {
            return Err(already_adopted(Some(&node.name)));
        }
        let Some(record) = adopted.iter().find(|node| node.id == replace.node) else {
            return Err(internal("finding the node to replace", &"it is gone"));
        };
        if replace.retrust && (record.mac != mac || record.board_name != board) {
            return Err(JobError {
                code: JobErrorCode::IdentityMismatch,
                message: format!(
                    "This is not {}'s device: that was a {} with MAC address {}, this one is a \
                     {board} with {mac}. Replace its device instead",
                    record.name, record.board_name, record.mac
                ),
            });
        }
        return Ok(Some(replace.node));
    }
    let Some(node) = found else {
        return Ok(None);
    };
    let approvals = shared
        .store
        .ztp_approvals(shared.clock.now())
        .await
        .map_err(|err| internal("reading the pre-approvals", &err))?;
    let Some(approval) = approvals
        .iter()
        .find(|approval| approval.mac == mac && approval.node == Some(node.id))
    else {
        return Err(already_adopted(Some(&node.name)));
    };
    if let Some(expected) = approval.board_name.as_deref().filter(|b| *b != board) {
        return Err(JobError {
            code: JobErrorCode::IdentityMismatch,
            message: format!(
                "{} awaits its device, a {expected}; this one with its MAC address is a {board}",
                node.name
            ),
        });
    }
    Ok(Some(node.id))
}

/// Records `adoption` in node `id`'s record and moves its settings to the new device; returns
/// the node and the step's detail.
pub(super) async fn take_over(
    shared: &Shared,
    id: NodeId,
    adoption: Adoption,
    by: Option<&Author>,
) -> Result<(Node, String), JobError> {
    let store = &shared.store;
    let old = store
        .node(id)
        .await
        .map_err(|err| internal("reading the node", &err))?
        .ok_or_else(|| internal("finding the node to replace", &"it is gone"))?;
    let old_facts = store
        .latest_facts(id)
        .await
        .map_err(|err| internal("reading the node's facts", &err))?
        .unwrap_or_default();
    let new_facts = adoption.facts.clone();
    let node = store
        .replace_node(id, adoption)
        .await
        .map_err(|err| match err {
            uniwrt_store::Error::Conflict { .. } => already_adopted(None),
            err => internal("recording the node", &err),
        })?
        .ok_or_else(|| internal("finding the node to replace", &"it is gone"))?;
    let same = old.mac == node.mac;
    let mut detail = if same {
        format!("Adopted back into {}'s record in observe mode", node.name)
    } else {
        format!(
            "Took over {}'s record (it was {}, {}) in observe mode",
            node.name, old.board_name, old.mac
        )
    };

    let old_device = Device::new(old.mac, &old_facts, port_names(old.role, &old_facts));
    let new_device = Device::new(node.mac, &new_facts, port_names(node.role, &new_facts));
    let mut dropped = Dropped::default();
    let message = if same {
        format!("Re-adopted {}", node.name)
    } else {
        format!("Replaced {}'s device", node.name)
    };
    match revise(shared, by, RevisionKind::Replace, &message, |config| {
        let replaced = replace_node(config, id, &old_device, &new_device);
        dropped = replaced.dropped;
        replaced.changed
    })
    .await
    {
        Ok(Some(revision)) => {
            let _ = write!(detail, "; revision {revision} moves its settings to it");
            for line in dropped.lines() {
                let _ = write!(detail, "; {line}");
            }
        }
        Ok(None) => {}
        Err(err) => {
            // The record is the new device's already: say what is left to do.
            warn!(node = %id, "cannot move the node's settings: {}", err.message);
            detail.push_str(
                "; its settings could not be moved to it (review its radios, ports and \
                 reservation before applying)",
            );
        }
    }
    Ok((node, detail))
}
