//! What drift shows of a package (pure): its diff from what the controller confirmed, secrets
//! masked, and whether two texts mean the same; with the hash `sha256sum` prints.

use std::collections::BTreeMap;

use uniwrt_core::NodeId;
use uniwrt_model::{
    apply::mask,
    drift::{Confirmed, Drift, DriftRecord},
};
use uniwrt_uci::Package;

pub(super) type Key = (NodeId, String);

/// The entries of `node`.
pub(super) fn of<T>(node: NodeId, map: &BTreeMap<Key, T>) -> impl Iterator<Item = (&Key, &T)> {
    map.range((node, String::new())..)
        .take_while(move |(key, _)| key.0 == node)
}

/// SHA-256 of `text`, lowercase hex, as `sha256sum` prints it.
pub(super) fn sha256(text: &str) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest as _, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Both texts parse and mean the same.
pub(super) fn same(package: &str, confirmed: &str, now: &str) -> bool {
    match (
        uniwrt_uci::parse(package, confirmed),
        uniwrt_uci::parse(package, now),
    ) {
        (Ok(a), Ok(b)) => uniwrt_uci::diff(&a, &b).is_empty(),
        _ => false,
    }
}

pub(super) fn view(confirmed: &Confirmed, record: &DriftRecord) -> Drift {
    let package = &record.package;
    let before =
        uniwrt_uci::parse(package, &confirmed.text).unwrap_or_else(|_| Package::new(package));
    let (after, error) = match &record.text {
        None => (Some(Package::new(package)), None),
        Some(text) => match uniwrt_uci::parse(package, text) {
            Ok(after) => (Some(after), None),
            Err(err) => (None, Some(format!("The node's file doesn't parse: {err}"))),
        },
    };
    let mut diff = match &after {
        Some(after) => uniwrt_uci::diff(&before, after),
        None => uniwrt_uci::diff(&before, &before),
    };
    mask(&mut diff);
    Drift {
        node: record.node,
        package: package.clone(),
        revision: confirmed.revision,
        detected_at: record.detected_at,
        changed_at: record.changed_at,
        acknowledged: record.acknowledged.clone(),
        removed: record.text.is_none(),
        error,
        diff,
    }
}
