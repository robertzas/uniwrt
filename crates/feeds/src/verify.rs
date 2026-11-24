//! Checking a downloaded package before the mirror keeps it: its size, then the hash its
//! feed's index gives (opkg's SHA-256 of the file; apk's of the package's control block, the
//! first block of its ADB stream, which is decompressed only that far). The node checks the
//! package's signature itself as it installs it, as from any feed.

use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use flate2::read::DeflateDecoder;
use sha2::{Digest as _, Sha256};

use crate::index::{Check, Entry, adb, hex};

/// The most of a package's control block read to check it.
const MAX_CONTROL: u64 = 64 << 20;

/// Why `path` isn't `entry`'s file, if it isn't. Blocking: run it off the runtime.
pub fn check(entry: &Entry, path: &Path) -> Result<(), String> {
    let size = std::fs::metadata(path)
        .map_err(|err| format!("{}: {err}", path.display()))?
        .len();
    if size != entry.size {
        return Err(format!(
            "{} is {size} bytes, the index says {}",
            entry.file, entry.size
        ));
    }
    let Some(check) = entry.check else {
        return Ok(());
    };
    let file =
        BufReader::new(File::open(path).map_err(|err| format!("{}: {err}", path.display()))?);
    let (expected, actual) = match check {
        Check::Sha256(expected) => {
            let mut hasher = Sha256::new();
            let mut reader = file;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = reader
                    .read(&mut buf)
                    .map_err(|err| format!("{}: {err}", entry.file))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            (expected, <[u8; 32]>::from(hasher.finalize()))
        }
        Check::ApkControl(expected) => (
            expected,
            control_hash(file).map_err(|why| format!("{}: {why}", entry.file))?,
        ),
    };
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "{}'s hash is {}, the index says {}",
            entry.file,
            hex(&actual),
            hex(&expected)
        ))
    }
}

/// The SHA-256 of an apk package's control block.
fn control_hash(mut file: impl Read) -> Result<[u8; 32], String> {
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).map_err(|err| err.to_string())?;
    let mut stream: Box<dyn Read> = match &magic {
        b"ADB." => Box::new(std::io::Cursor::new(magic.to_vec()).chain(file)),
        b"ADBd" => Box::new(DeflateDecoder::new(file)),
        _ => return Err("not an apk v3 package".to_owned()),
    };
    // The stream's header and the first block's 4-byte header.
    let mut head = [0u8; 12];
    stream
        .read_exact(&mut head)
        .map_err(|err| err.to_string())?;
    if &head[..4] != b"ADB." || head[4..8] != adb::SCHEMA_PACKAGE {
        return Err("not an apk package's stream".to_owned());
    }
    let first = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    if first >> 30 != 0 {
        return Err("the first block isn't the control block".to_owned());
    }
    let len = u64::from(first & 0x3fff_ffff)
        .checked_sub(4)
        .filter(|&len| len <= MAX_CONTROL)
        .ok_or("the control block's size")?;
    let mut hasher = Sha256::new();
    let copied = std::io::copy(&mut stream.take(len), &mut HashWriter(&mut hasher))
        .map_err(|err| err.to_string())?;
    if copied != len {
        return Err("the control block is cut short".to_owned());
    }
    Ok(hasher.finalize().into())
}

struct HashWriter<'a>(&'a mut Sha256);

impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write as _;

    use flate2::{Compression, write::DeflateEncoder};

    use super::*;

    /// A package as apk-tools writes one: `ADBd`, then the deflated stream of a control
    /// block and a data block.
    pub(crate) fn apk_package(control: &[u8]) -> Vec<u8> {
        let mut stream = b"ADB.pckg".to_vec();
        let block = |stream: &mut Vec<u8>, kind: u32, payload: &[u8]| {
            let size = u32::try_from(payload.len() + 4).unwrap();
            stream.extend_from_slice(&(kind << 30 | size).to_le_bytes());
            stream.extend_from_slice(payload);
            while !stream.len().is_multiple_of(8) {
                stream.push(0);
            }
        };
        block(&mut stream, 0, control);
        block(&mut stream, 2, b"file contents");
        let mut out = b"ADBd".to_vec();
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&stream).unwrap();
        out.extend(encoder.finish().unwrap());
        out
    }

    fn entry(file: &[u8], check: Check) -> Entry {
        Entry {
            name: "x".to_owned(),
            version: "1".to_owned(),
            file: "x-1.apk".to_owned(),
            size: file.len() as u64,
            check: Some(check),
            depends: Vec::new(),
            provides: Vec::new(),
        }
    }

    #[test]
    fn files_are_checked_by_size_and_the_index_s_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        let control = b"control block, as apk hashes it";
        let package = apk_package(control);
        std::fs::write(&path, &package).unwrap();
        let good = entry(&package, Check::ApkControl(Sha256::digest(control).into()));
        assert_eq!(check(&good, &path), Ok(()));
        let other = entry(&package, Check::ApkControl([7; 32]));
        assert!(check(&other, &path).unwrap_err().contains("hash"));
        let whole = entry(&package, Check::Sha256(Sha256::digest(&package).into()));
        assert_eq!(check(&whole, &path), Ok(()));
        let mut short = good.clone();
        short.size += 1;
        assert!(check(&short, &path).unwrap_err().contains("bytes"));
        std::fs::write(&path, vec![b'x'; package.len()]).unwrap();
        assert!(check(&good, &path).is_err());
    }
}
