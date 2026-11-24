//! The UCI explorer's read of a node (features/observability.md, Diagnostics toolkit › UCI
//! explorer; D-141, D-143): every package in its `/etc/config` as the node holds it now, with
//! the values of secret options masked, who owns each package, and which of the node's raw
//! sections in the site's latest revision find each section.

use serde::{Deserialize, Serialize};
use uniwrt_core::{NodeId, RevisionId, Timestamp};
use uniwrt_uci::Value;

use crate::site::node::is_secret_option;

/// `GET v1/nodes/{id}/uci`: the node's packages, read live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NodeUci {
    pub node: NodeId,
    pub read_at: Timestamp,
    /// The site's revision whose node's raw sections `overrides` and `unmatched` count in
    /// (`NodeConfig::raw`'s indices); `None` before the site has one.
    pub revision: Option<RevisionId>,
    /// By name.
    pub packages: Vec<UciTreePackage>,
    /// The node's raw sections that find no section here: not applied yet, a removal applied
    /// already, or their package missing on the node.
    pub unmatched: Vec<u32>,
}

/// Who writes a package on the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UciOwner {
    /// The controller renders it whole (a managed package of a managed node).
    Rendered,
    /// The node's own file, which the node's raw sections patch (D-143).
    Patched,
    /// The node's own: the controller doesn't touch it (every package in observe mode).
    Node,
}

/// One `/etc/config` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UciTreePackage {
    pub name: String,
    pub owner: UciOwner,
    /// In file order; empty when it can't be read.
    pub sections: Vec<UciTreeSection>,
    /// Why it can't be read (not UTF-8, or `uci` couldn't load it, in its words).
    pub error: Option<String>,
}

/// A section as the node holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UciTreeSection {
    #[serde(rename = "type")]
    pub kind: String,
    /// `None` for an anonymous one, which `uci` addresses as `@type[index]`.
    pub name: Option<String>,
    /// In file order.
    pub options: Vec<UciTreeOption>,
    /// The node's raw sections that find it (indices, in the site's order).
    pub overrides: Vec<u32>,
    /// The options a merge or removal finds an anonymous section of its type by when it matches
    /// none (render's `raw::key`, such as a zone's `name`; empty for a type a package holds
    /// once: its first section); `None` for a type without them.
    pub key: Option<Vec<String>>,
}

/// An option or a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct UciTreeOption {
    pub name: String,
    /// A secret's value (or each of its items) reads [`MASKED`](crate::apply::MASKED).
    pub value: Value,
    /// It holds a secret ([`is_masked_option`]).
    pub secret: bool,
}

/// The words in an option's name that say it holds a secret in a package render doesn't know.
const SECRET_WORDS: [&str; 9] = [
    "passw",
    "passphrase",
    "secret",
    "psk",
    "token",
    "private",
    "credential",
    "apikey",
    "api_key",
];

/// Whether the explorer masks an option of that name: one render knows holds a secret
/// ([`is_secret_option`]), or one whose name says so in any package (`password`, `passwd`,
/// `secret`, `psk`, `token`, a private key, credentials, an API key), as the explorer shows
/// packages render doesn't know.
pub fn is_masked_option(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    is_secret_option(name) || SECRET_WORDS.iter().any(|word| lower.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_masked_by_render_s_rule_and_by_their_names() {
        for name in [
            "key",
            "sae_password",
            "r0kh",
            "passwd",
            "PassPhrase",
            "secret_key",
        ] {
            assert!(is_masked_option(name), "{name}");
        }
        for name in [
            "psk_file",
            "auth_token",
            "privkey_private",
            "api_key",
            "credentials",
        ] {
            assert!(is_masked_option(name), "{name}");
        }
        for name in ["ssid", "public_key", "encryption", "bypass", "keepalive"] {
            assert!(!is_masked_option(name), "{name}");
        }
    }
}
