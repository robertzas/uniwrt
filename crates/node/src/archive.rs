//! What a configuration backup holds (D-126): the gzip-compressed tar `sysupgrade -b` writes,
//! read without touching any disk. [`files`] lists its regular files, [`content_digest`] says
//! whether two backups hold the same files whatever their times.

use std::{collections::BTreeMap, fmt::Write as _, io::Read as _};

use flate2::read::GzDecoder;
use ring::digest;

/// The most a backup may hold unpacked: `sysupgrade -b` archives are tens of KiB.
const MAX_UNPACKED: u64 = 64 << 20;

const BLOCK: usize = 512;

/// One entry of the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its path, without a leading `/` or `./`.
    pub path: String,
    /// The tar type flag: `b'0'` for a regular file, `b'5'` a directory, `b'2'` a symlink.
    pub kind: u8,
    pub mode: u32,
    pub link: String,
    pub data: Vec<u8>,
}

impl Entry {
    pub fn is_file(&self) -> bool {
        self.kind == b'0' || self.kind == 0
    }
}

/// Why an archive can't be read.
#[derive(Debug, thiserror::Error)]
#[error("not a readable backup archive: {0}")]
pub struct ArchiveError(String);

fn bad(why: impl Into<String>) -> ArchiveError {
    ArchiveError(why.into())
}

/// The archive's entries, in its order; GNU long names (`L`, `K`) are resolved and pax headers
/// skipped.
pub fn entries(tarball: &[u8]) -> Result<Vec<Entry>, ArchiveError> {
    let mut tar = Vec::new();
    GzDecoder::new(tarball)
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut tar)
        .map_err(|err| bad(err.to_string()))?;
    if tar.len() as u64 > MAX_UNPACKED {
        return Err(bad("larger than 64 MiB unpacked"));
    }
    let mut entries = Vec::new();
    let (mut long_name, mut long_link) = (None, None);
    let mut at = 0;
    while at + BLOCK <= tar.len() {
        let header = &tar[at..at + BLOCK];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let size = usize::try_from(octal(&header[124..136])?).map_err(|_| bad("entry size"))?;
        let start = at + BLOCK;
        let end = start.checked_add(size).ok_or_else(|| bad("entry size"))?;
        let data = tar.get(start..end).ok_or_else(|| bad("truncated"))?;
        at = start + size.div_ceil(BLOCK) * BLOCK;
        match header[156] {
            b'L' => long_name = Some(text(data)),
            b'K' => long_link = Some(text(data)),
            b'x' | b'g' => {}
            kind => {
                let path = long_name.take().unwrap_or_else(|| {
                    let name = text(&header[0..100]);
                    let prefix = if &header[257..262] == b"ustar" {
                        text(&header[345..500])
                    } else {
                        String::new()
                    };
                    if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}/{name}")
                    }
                });
                let mut trimmed = path.as_str();
                while let Some(rest) = trimmed
                    .strip_prefix("./")
                    .or_else(|| trimmed.strip_prefix('/'))
                {
                    trimmed = rest;
                }
                let path = trimmed.to_owned();
                entries.push(Entry {
                    path,
                    kind,
                    mode: u32::try_from(octal(&header[100..108])? & 0o7777).unwrap_or(0),
                    link: long_link.take().unwrap_or_else(|| text(&header[157..257])),
                    data: data.to_vec(),
                });
            }
        }
    }
    Ok(entries)
}

/// The UCI packages the archive holds: `etc/config/<name>` files, by name.
pub fn config_files(entries: &[Entry]) -> BTreeMap<String, &[u8]> {
    entries
        .iter()
        .filter(|entry| entry.is_file())
        .filter_map(|entry| {
            let name = entry.path.strip_prefix("etc/config/")?;
            (!name.is_empty() && !name.contains('/')).then(|| (name.to_owned(), &entry.data[..]))
        })
        .collect()
}

/// SHA-256 over every entry's path, kind, mode, link and data, in path order: equal for two
/// backups of the same files, whatever their times, owners or order (`sysupgrade -b` writes
/// the mtimes of files touched but unchanged, such as `/etc/hosts`).
pub fn content_digest(tarball: &[u8]) -> Result<String, ArchiveError> {
    let mut entries = entries(tarball)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let mut context = digest::Context::new(&digest::SHA256);
    for entry in &entries {
        for part in [entry.path.as_bytes(), entry.link.as_bytes(), &entry.data] {
            context.update(&(part.len() as u64).to_le_bytes());
            context.update(part);
        }
        context.update(&[entry.kind]);
        context.update(&entry.mode.to_le_bytes());
    }
    Ok(hex(context.finish().as_ref()))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A NUL-terminated field.
fn text(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A numeric field: octal digits, or GNU's base-256 when its first byte has the high bit.
fn octal(field: &[u8]) -> Result<u64, ArchiveError> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return Ok(field[1..]
            .iter()
            .fold(0u64, |n, &b| n.wrapping_shl(8) | u64::from(b)));
    }
    let digits = text(field);
    let digits = digits.trim_matches(|c: char| c == ' ' || c == '\0');
    if digits.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(digits, 8).map_err(|_| bad(format!("number {digits:?}")))
}

/// A ustar header block.
fn header(path: &str, kind: u8, mode: u32, size: usize, mtime: u64) -> [u8; BLOCK] {
    let mut h = [0u8; BLOCK];
    let name = &path.as_bytes()[..path.len().min(100)];
    h[..name.len()].copy_from_slice(name);
    h[100..107].copy_from_slice(format!("{:07o}", mode & 0o7777).as_bytes());
    h[108..115].copy_from_slice(b"0000000");
    h[116..123].copy_from_slice(b"0000000");
    h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
    h[136..147].copy_from_slice(format!("{:011o}", mtime & 0o777_7777_7777).as_bytes());
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
    h
}

/// A gzip-compressed tar of regular files (path, content), each with mode 0600 and `mtime`, as
/// `sysupgrade -b` writes them: for the simulated nodes and tests. Paths over 100 bytes are cut.
pub fn pack(files: &[(&str, &[u8])], mtime: u64) -> Vec<u8> {
    use std::io::Write as _;

    let mut tar = Vec::new();
    for (path, content) in files {
        tar.extend_from_slice(&header(path, b'0', 0o600, content.len(), mtime));
        tar.extend_from_slice(content);
        tar.resize(tar.len().div_ceil(BLOCK) * BLOCK, 0);
    }
    tar.resize(tar.len() + 2 * BLOCK, 0);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing into a Vec can't fail.
    let _ = gz.write_all(&tar);
    gz.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    fn tarball(files: &[(&str, &str)], mtime: u64) -> Vec<u8> {
        let files: Vec<(&str, &[u8])> = files.iter().map(|(p, c)| (*p, c.as_bytes())).collect();
        pack(&files, mtime)
    }

    #[test]
    fn the_entries_and_config_files_are_read() {
        let tarball = tarball(
            &[
                ("etc/config/network", "config interface 'lan'\n"),
                ("etc/passwd", "root:x:0:0\n"),
                ("./etc/config/wireless", "config wifi-device 'radio0'\n"),
            ],
            1,
        );
        let entries = entries(&tarball).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].path, "etc/passwd");
        assert_eq!(entries[1].mode, 0o600);
        let config = config_files(&entries);
        assert_eq!(
            config.keys().collect::<Vec<_>>(),
            ["network", "wireless"],
            "./ is dropped, etc/passwd isn't config"
        );
        assert_eq!(config["network"], b"config interface 'lan'\n");
    }

    #[test]
    fn the_digest_ignores_times_and_order_but_not_content() {
        let a = tarball(&[("etc/hosts", "a"), ("etc/config/dhcp", "x")], 1);
        let later = tarball(&[("etc/config/dhcp", "x"), ("etc/hosts", "a")], 999);
        let other = tarball(&[("etc/hosts", "b"), ("etc/config/dhcp", "x")], 1);
        assert_ne!(a, later);
        assert_eq!(content_digest(&a).unwrap(), content_digest(&later).unwrap());
        assert_ne!(content_digest(&a).unwrap(), content_digest(&other).unwrap());
    }

    #[test]
    fn long_names_resolve_and_garbage_is_refused() {
        let long = format!("etc/{}", "n".repeat(120));
        let mut tar = Vec::new();
        tar.extend_from_slice(&header("././@LongLink", b'L', 0, long.len(), 0));
        tar.extend_from_slice(long.as_bytes());
        tar.resize(tar.len().div_ceil(BLOCK) * BLOCK, 0);
        tar.extend_from_slice(&header("etc/nnnn", b'0', 0o644, 1, 0));
        tar.extend_from_slice(b"z");
        tar.resize(tar.len().div_ceil(BLOCK) * BLOCK + 2 * BLOCK, 0);
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&tar).unwrap();
        let entries = entries(&gz.finish().unwrap()).unwrap();
        assert_eq!(entries[0].path, long);
        assert_eq!(entries[0].data, b"z");

        assert!(entries_of(b"not gzip").is_err());
        let mut truncated = tarball(&[("etc/hosts", "abc")], 0);
        truncated.truncate(truncated.len() - 12);
        assert!(entries_of(&truncated).is_err());
    }

    fn entries_of(bytes: &[u8]) -> Result<Vec<Entry>, ArchiveError> {
        entries(bytes)
    }
}
