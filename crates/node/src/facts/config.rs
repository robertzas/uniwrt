//! The `config` set: the SHA-256 of each UCI package in `/etc/config`, by package
//! (`node-agent/collect/config.uc`). A hash that isn't 64 lowercase hex digits is skipped.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    files: BTreeMap<String, String>,
}

pub(super) fn parse(raw: Raw) -> BTreeMap<String, String> {
    raw.files
        .into_iter()
        .filter(|(_, hash)| {
            hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .collect()
}
