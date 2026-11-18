//! The compiler: lists in, one dnsmasq `servers-file` out (D-133). Every list's domains
//! ([`parse`](crate::parse)), deduplicated; those allowed (with their subdomains) removed and
//! the user's own blocks added; a domain under another blocked one dropped (dnsmasq blocks the
//! subdomains already); then, when the gateway can't hold them all, the least wanted cut: the
//! user's own blocks come first, then the lists in the order given. An allowed domain under a
//! blocked one gets a `server=/<domain>/#` line, which sends it to the usual upstream.
//!
//! The file is the [`header`] line, then one `server=/<domain>/` (or `…/#`) line per domain,
//! sorted, so the same lists compile to the same SHA-256 and the gateway is left alone.

use std::collections::{HashMap, HashSet};

use uniwrt_model::blocklists::header;

use crate::parse;

/// A compiled list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// The whole file: the [`header`], then the lines.
    pub text: Vec<u8>,
    /// The SHA-256 of the lines after the header (hex).
    pub sha256: String,
    /// The domains blocked.
    pub domains: u32,
    /// The domains left out for `capacity`.
    pub cut: u32,
}

/// Compiles `lists` (their bodies, most wanted first), `allow` and `deny` into at most
/// `capacity` blocked domains.
pub fn compile(lists: &[&[u8]], allow: &[String], deny: &[String], capacity: u32) -> Compiled {
    let allow: HashSet<String> = allow
        .iter()
        .filter_map(|name| parse::domain(name))
        .collect();
    // Each domain's rank: 0 for the user's own, then the list's position from 1.
    let mut blocked: HashMap<String, usize> = deny
        .iter()
        .filter_map(|name| parse::domain(name))
        .map(|name| (name, 0))
        .collect();
    let mut names = Vec::new();
    for (i, body) in lists.iter().enumerate() {
        for line in body.split(|&b| b == b'\n') {
            parse::line(line, &mut names);
            for name in names.drain(..) {
                if !within(&name, &allow) {
                    blocked.entry(name).or_insert(i + 1);
                }
            }
        }
    }
    let kept = topmost(&blocked);
    drop(blocked);
    let (kept, cut) = cap(kept, capacity);
    let held: HashSet<&str> = kept.iter().map(|(name, _)| name.as_str()).collect();
    let mut lines: Vec<(&str, bool)> = held.iter().map(|&name| (name, false)).collect();
    lines.extend(
        allow
            .iter()
            .filter(|name| ancestors(name).any(|parent| held.contains(parent)))
            .map(|name| (name.as_str(), true)),
    );
    lines.sort_unstable();
    let mut body = Vec::with_capacity(lines.len() * 32);
    for (name, allowed) in &lines {
        body.extend_from_slice(b"server=/");
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(if *allowed { b"/#\n" } else { b"/\n" });
    }
    let sha256 = sha256(&body);
    let domains = u32::try_from(held.len()).unwrap_or(u32::MAX);
    let mut text = header(&sha256, domains).into_bytes();
    text.push(b'\n');
    text.extend_from_slice(&body);
    Compiled {
        text,
        sha256,
        domains,
        cut,
    }
}

/// The domains no other blocked domain contains, each ranked as the best of those it contains.
fn topmost(blocked: &HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut raised: HashMap<&str, usize> = HashMap::new();
    let mut kept = Vec::with_capacity(blocked.len());
    for (name, &rank) in blocked {
        // The shortest blocked parent is a topmost one.
        match ancestors(name).filter(|p| blocked.contains_key(*p)).last() {
            Some(parent) => {
                let best = raised.entry(parent).or_insert(rank);
                *best = (*best).min(rank);
            }
            None => kept.push((name.clone(), rank)),
        }
    }
    for (name, rank) in &mut kept {
        if let Some(&best) = raised.get(name.as_str()) {
            *rank = (*rank).min(best);
        }
    }
    kept
}

/// The `capacity` best ranked domains, and how many were cut.
fn cap(mut kept: Vec<(String, usize)>, capacity: u32) -> (Vec<(String, usize)>, u32) {
    let capacity = usize::try_from(capacity).unwrap_or(usize::MAX);
    if kept.len() <= capacity {
        return (kept, 0);
    }
    kept.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let cut = u32::try_from(kept.len() - capacity).unwrap_or(u32::MAX);
    kept.truncate(capacity);
    (kept, cut)
}

/// Whether `name` is in `set` or under a domain in it.
fn within(name: &str, set: &HashSet<String>) -> bool {
    !set.is_empty() && (set.contains(name) || ancestors(name).any(|p| set.contains(p)))
}

/// `name`'s parent domains, nearest first (`a.b.c` → `b.c`, `c`).
fn ancestors(name: &str) -> impl Iterator<Item = &str> {
    name.match_indices('.').map(move |(i, _)| &name[i + 1..])
}

fn sha256(data: &[u8]) -> String {
    use std::fmt::Write as _;

    use sha2::{Digest as _, Sha256};
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
#[path = "compile_tests.rs"]
mod tests;
