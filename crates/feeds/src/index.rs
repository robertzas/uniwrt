//! A feed's index, read the way the node's package manager reads it: opkg's `Packages.gz`
//! ([`opkg`]) or apk's `packages.adb` ([`adb`]). The mirror needs each package's file, its
//! size and the hash that checks it, and the names it depends on and provides, to follow
//! dependencies ([`crate::closure`]).

pub mod adb;
pub mod opkg;
#[cfg(test)]
mod tests;

use uniwrt_model::facts::PackageManager;

use crate::Error;

/// The most an index may take, as downloaded.
pub const MAX_INDEX: u64 = 64 << 20;
/// The most an index may take once decompressed.
pub(crate) const MAX_INFLATED: u64 = 256 << 20;

/// What checks a package's file, besides its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The SHA-256 of the whole file (opkg's `SHA256sum`).
    Sha256([u8; 32]),
    /// The SHA-256 of the package's control block, the first block of its ADB stream (apk's
    /// index records this, not the file's).
    ApkControl([u8; 32]),
}

/// One package of a feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub version: String,
    /// Its file, in the feed's directory (one path segment).
    pub file: String,
    /// Its file's size, in bytes.
    pub size: u64,
    pub check: Option<Check>,
    /// What it depends on: each item is a list of alternatives (opkg's `a | b`), by name.
    pub depends: Vec<Vec<String>>,
    /// The other names it answers to.
    pub provides: Vec<String>,
}

/// A feed's packages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub entries: Vec<Entry>,
}

/// The files of a feed's index for `manager`, the one [`parse`] reads first. opkg fetches
/// `Packages.gz` and its signature, `Packages.sig`; apk's index carries its signature.
pub fn files(manager: PackageManager) -> &'static [&'static str] {
    match manager {
        PackageManager::Opkg => &["Packages.gz", "Packages.sig"],
        PackageManager::Apk => &["packages.adb"],
    }
}

/// Reads `manager`'s index from the first of its [`files`].
pub fn parse(manager: PackageManager, bytes: &[u8]) -> Result<Index, Error> {
    match manager {
        PackageManager::Opkg => opkg::parse(bytes),
        PackageManager::Apk => adb::parse(bytes),
    }
}

/// Lowercase hexadecimal.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

/// The 32 bytes `text` spells in hexadecimal.
pub(crate) fn unhex32(text: &str) -> Option<[u8; 32]> {
    let text = text.as_bytes();
    if text.len() != 64 {
        return None;
    }
    let digit = |b: u8| {
        char::from(b)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let mut out = [0; 32];
    let (pairs, _) = text.as_chunks::<2>();
    for (i, pair) in pairs.iter().enumerate() {
        out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}
