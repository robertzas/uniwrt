//! opkg's index: `Packages.gz`, a gzipped list of stanzas (`Package:`, `Version:`,
//! `Depends:`, `Provides:`, `Filename:`, `Size:`, `SHA256sum:`; a line starting with a
//! space continues the field before it). A stanza without a file name or size can't be
//! mirrored and is left out, as is one whose file isn't a plain name in the feed's directory.

use std::io::Read as _;

use flate2::read::GzDecoder;

use super::{Check, Entry, Index, MAX_INFLATED, unhex32};
use crate::{Error, feed::valid_segment};

/// Reads a `Packages.gz`.
pub fn parse(gzipped: &[u8]) -> Result<Index, Error> {
    let mut text = String::new();
    GzDecoder::new(gzipped)
        .take(MAX_INFLATED)
        .read_to_string(&mut text)
        .map_err(|err| Error::Invalid(format!("Packages.gz: {err}")))?;
    Ok(parse_text(&text))
}

/// Reads the stanzas of an uncompressed `Packages`.
pub fn parse_text(text: &str) -> Index {
    let mut entries = Vec::new();
    for stanza in text.split("\n\n") {
        if let Some(entry) = stanza_entry(stanza) {
            entries.push(entry);
        }
    }
    Index { entries }
}

fn stanza_entry(stanza: &str) -> Option<Entry> {
    let (mut name, mut version, mut file, mut size, mut check) = (None, None, None, None, None);
    let (mut depends, mut provides) = (Vec::new(), Vec::new());
    for line in stanza.lines() {
        if line.starts_with(' ') {
            continue;
        }
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match field {
            "Package" => name = Some(value.to_owned()),
            "Version" => version = Some(value.to_owned()),
            "Filename" => file = Some(value.to_owned()),
            "Size" => size = value.parse::<u64>().ok(),
            "SHA256sum" => check = unhex32(value).map(Check::Sha256),
            "Depends" => depends = alternatives(value),
            "Provides" => provides = value.split(',').filter_map(bare_name).collect(),
            _ => {}
        }
    }
    let file = file.filter(|file| valid_segment(file))?;
    Some(Entry {
        name: name.filter(|name| !name.is_empty())?,
        version: version.unwrap_or_default(),
        file,
        size: size?,
        check,
        depends,
        provides,
    })
}

/// `libc, libubox20240329 (>= 2024.03.29), a | b` as lists of alternatives, by name.
fn alternatives(value: &str) -> Vec<Vec<String>> {
    value
        .split(',')
        .map(|item| item.split('|').filter_map(bare_name).collect::<Vec<_>>())
        .filter(|alternatives| !alternatives.is_empty())
        .collect()
}

/// A dependency's name, without its version constraint (`name (>= 1)`, `name=1`).
fn bare_name(item: &str) -> Option<String> {
    let name = item
        .trim()
        .split([' ', '(', '=', '<', '>'])
        .next()
        .unwrap_or_default();
    (!name.is_empty()).then(|| name.to_owned())
}
