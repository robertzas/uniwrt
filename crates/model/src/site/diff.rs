//! The structural diff of two documents: what was added, removed or changed, by path. List items
//! are matched by their key (`id`, `node`, `radio`, `mac`, `port` or `name`), so editing one network is one
//! change at `networks/<id>/…`, not a shifted list. Lists without a key compare whole.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::SiteConfig;

/// The fields that key list items, in the order they are tried.
const KEYS: [&str; 6] = ["id", "node", "radio", "mac", "port", "name"];

/// One difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Change {
    /// Field names, and list items by key, such as `["networks", "iot", "vlan"]`.
    pub path: Vec<String>,
    pub op: ChangeOp,
    /// The value before; `None` when added.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub before: Option<Value>,
    /// The value after; `None` when removed.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub after: Option<Value>,
}

/// What a [`Change`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ChangeOp {
    Added,
    Removed,
    Changed,
}

/// The changes that turn `before` into `after`, in document order.
pub fn diff(before: &SiteConfig, after: &SiteConfig) -> Vec<Change> {
    let mut changes = Vec::new();
    // A SiteConfig always serializes: its maps have string keys.
    if let (Ok(before), Ok(after)) = (serde_json::to_value(before), serde_json::to_value(after)) {
        values(&mut Vec::new(), &before, &after, &mut changes);
    }
    changes
}

fn values(path: &mut Vec<String>, before: &Value, after: &Value, out: &mut Vec<Change>) {
    if before == after {
        return;
    }
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => objects(path, before, after, out),
        (Value::Array(before), Value::Array(after)) => match key_of(before, after) {
            Some(key) => keyed(path, key, before, after, out),
            None => out.push(changed(path, before.clone().into(), after.clone().into())),
        },
        _ => out.push(changed(path, before.clone(), after.clone())),
    }
}

fn objects(
    path: &mut Vec<String>,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    out: &mut Vec<Change>,
) {
    let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for name in names {
        path.push(name.clone());
        match (before.get(name), after.get(name)) {
            (Some(before), Some(after)) => values(path, before, after, out),
            (Some(before), None) => out.push(removed(path, before.clone())),
            (None, Some(after)) => out.push(added(path, after.clone())),
            (None, None) => {}
        }
        path.pop();
    }
}

/// Items matched by `key`: changes to the ones on both sides, then the removed ones, then the
/// added ones, each in its side's order.
fn keyed(
    path: &mut Vec<String>,
    key: &str,
    before: &[Value],
    after: &[Value],
    out: &mut Vec<Change>,
) {
    let find = |items: &'_ [Value], id: &str| -> Option<Value> {
        items
            .iter()
            .find(|item| item_key(item, key).as_deref() == Some(id))
            .cloned()
    };
    for item in before {
        let Some(id) = item_key(item, key) else {
            continue;
        };
        path.push(id.clone());
        match find(after, &id) {
            Some(other) => values(path, item, &other, out),
            None => out.push(removed(path, item.clone())),
        }
        path.pop();
    }
    for item in after {
        let Some(id) = item_key(item, key) else {
            continue;
        };
        if find(before, &id).is_none() {
            path.push(id);
            out.push(added(path, item.clone()));
            path.pop();
        }
    }
}

/// The key every item on both sides has, unique on each side, if any.
fn key_of(before: &[Value], after: &[Value]) -> Option<&'static str> {
    KEYS.into_iter().find(|key| {
        [before, after].into_iter().all(|items| {
            let ids: Option<BTreeSet<String>> =
                items.iter().map(|item| item_key(item, key)).collect();
            ids.is_some_and(|ids| ids.len() == items.len())
        })
    })
}

fn item_key(item: &Value, key: &str) -> Option<String> {
    match item.get(key)? {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

fn added(path: &[String], after: Value) -> Change {
    Change {
        path: path.to_vec(),
        op: ChangeOp::Added,
        before: None,
        after: Some(after),
    }
}

fn removed(path: &[String], before: Value) -> Change {
    Change {
        path: path.to_vec(),
        op: ChangeOp::Removed,
        before: Some(before),
        after: None,
    }
}

fn changed(path: &[String], before: Value, after: Value) -> Change {
    Change {
        path: path.to_vec(),
        op: ChangeOp::Changed,
        before: Some(before),
        after: Some(after),
    }
}
