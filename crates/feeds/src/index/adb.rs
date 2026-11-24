//! apk's binary database format (ADB, apk-tools 3), as far as the mirror reads it: a feed's
//! index (`packages.adb`, schema `indx`) and a package's control block (schema `pckg`).
//!
//! A file is `ADB.` (uncompressed), `ADBd` (raw deflate) or `ADBc` plus a two-byte
//! compression spec, then the stream: `ADB.`, the schema, and blocks, each a little-endian
//! `u32` whose top two bits are its type (`ADB` 0, `SIG` 1, `DATA` 2; 3 is an extended
//! header of 16 bytes with a 64-bit size) and the rest its size including that header, padded
//! to 8 bytes. The first block holds the database: a header (versions, then the root value)
//! and values, each a `u32` whose top four bits are its type and the rest a number or an
//! offset into the block. Objects and arrays are a count (including itself) then that many
//! values less one; an object's slots are numbered by its schema (`apk_adb.h`).

use std::io::Read as _;

use flate2::read::DeflateDecoder;

use super::{Check, Entry, Index, MAX_INFLATED};
use crate::{Error, feed::valid_segment};

const SCHEMA_INDEX: [u8; 4] = *b"indx";
pub(crate) const SCHEMA_PACKAGE: [u8; 4] = *b"pckg";

const TYPE_SPECIAL: u32 = 0x0000_0000;
const TYPE_INT: u32 = 0x1000_0000;
const TYPE_INT_32: u32 = 0x2000_0000;
const TYPE_INT_64: u32 = 0x3000_0000;
const TYPE_BLOB_8: u32 = 0x8000_0000;
const TYPE_BLOB_16: u32 = 0x9000_0000;
const TYPE_BLOB_32: u32 = 0xa000_0000;
const TYPE_ARRAY: u32 = 0xd000_0000;
const TYPE_OBJECT: u32 = 0xe000_0000;
const TYPE_MASK: u32 = 0xf000_0000;
const VALUE_MASK: u32 = 0x0fff_ffff;
/// apk's limit on an object's or array's slots.
const MAX_SLOTS: u32 = 0x0100_0000;

/// The index's slots.
const NDX_PACKAGES: usize = 2;
const NDX_PKGNAME_SPEC: usize = 3;
/// A package's slots.
const PI_NAME: usize = 1;
const PI_VERSION: usize = 2;
const PI_HASHES: usize = 3;
const PI_ARCH: usize = 5;
const PI_FILE_SIZE: usize = 13;
const PI_DEPENDS: usize = 15;
const PI_PROVIDES: usize = 16;
/// A dependency's slots.
const DEP_NAME: usize = 1;
const DEP_MATCH: usize = 3;
/// A dependency that is a conflict (`!name`), not a need.
const MATCH_CONFLICT: u64 = 16;

/// The file name apk gives a package when the index names no other (`pkgname_spec`).
const DEFAULT_SPEC: &str = "${name}-${version}.apk";

fn invalid(why: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("ADB: {why}"))
}

/// The decompressed stream of an ADB file, from its `ADB.` header.
pub(crate) fn inflate(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let body = match bytes.get(..4) {
        Some(b"ADB.") => return Ok(bytes.to_vec()),
        Some(b"ADBd") => &bytes[4..],
        // A compression spec follows: the algorithm (none, deflate, zstd), then the level.
        Some(b"ADBc") => match bytes.get(4) {
            Some(0) => return Ok(bytes.get(6..).unwrap_or_default().to_vec()),
            Some(1) => bytes.get(6..).unwrap_or_default(),
            Some(alg) => return Err(invalid(format!("compression {alg} isn't supported"))),
            None => return Err(invalid("truncated")),
        },
        _ => return Err(invalid("not an ADB file")),
    };
    let mut out = Vec::new();
    DeflateDecoder::new(body)
        .take(MAX_INFLATED)
        .read_to_end(&mut out)
        .map_err(|err| invalid(format!("deflate: {err}")))?;
    Ok(out)
}

fn u32_at(data: &[u8], at: usize) -> Result<u32, Error> {
    let bytes = data
        .get(at..at.checked_add(4).ok_or_else(|| invalid("offset"))?)
        .ok_or_else(|| invalid("value out of bounds"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// The payload of the stream's first block, which must be the database, after checking the
/// stream's schema.
pub(crate) fn first_block(stream: &[u8], schema: [u8; 4]) -> Result<&[u8], Error> {
    if stream.get(..4) != Some(b"ADB.") || stream.get(4..8) != Some(schema.as_slice()) {
        return Err(invalid("unexpected header or schema"));
    }
    let head = u32_at(stream, 8)?;
    let (kind, raw, header) = if head >> 30 == 3 {
        let size = stream
            .get(16..24)
            .ok_or_else(|| invalid("truncated block"))?;
        let mut eight = [0; 8];
        eight.copy_from_slice(size);
        (head & 0x3fff_ffff, u64::from_le_bytes(eight), 16)
    } else {
        (head >> 30, u64::from(head & 0x3fff_ffff), 4)
    };
    if kind != 0 {
        return Err(invalid("the first block isn't the database"));
    }
    let end = usize::try_from(raw)
        .ok()
        .and_then(|raw| raw.checked_add(8))
        .ok_or_else(|| invalid("block size"))?;
    if raw < header {
        return Err(invalid("block size"));
    }
    stream
        .get(8 + usize::try_from(header).unwrap_or(0)..end)
        .ok_or_else(|| invalid("truncated block"))
}

/// A value of the database.
enum Value<'a> {
    Null,
    Int(u64),
    Blob(&'a [u8]),
    /// An object's or array's values, slot 1 first.
    Slots(Vec<u32>),
}

struct Db<'a> {
    data: &'a [u8],
}

impl<'a> Db<'a> {
    fn value(&self, raw: u32) -> Result<Value<'a>, Error> {
        let at = usize::try_from(raw & VALUE_MASK).map_err(|_| invalid("offset"))?;
        let data = self.data;
        let blob = |start: usize, len: usize| {
            data.get(start..start.checked_add(len).ok_or_else(|| invalid("blob"))?)
                .map(Value::Blob)
                .ok_or_else(|| invalid("blob out of bounds"))
        };
        match raw & TYPE_MASK {
            TYPE_SPECIAL => Ok(Value::Null),
            TYPE_INT => Ok(Value::Int(u64::from(raw & VALUE_MASK))),
            TYPE_INT_32 => Ok(Value::Int(u64::from(u32_at(data, at)?))),
            TYPE_INT_64 => {
                let low = u64::from(u32_at(data, at)?);
                let high = u64::from(u32_at(data, at + 4)?);
                Ok(Value::Int(high << 32 | low))
            }
            TYPE_BLOB_8 => blob(
                at + 1,
                usize::from(*data.get(at).ok_or_else(|| invalid("blob"))?),
            ),
            TYPE_BLOB_16 => {
                let len = data.get(at..at + 2).ok_or_else(|| invalid("blob"))?;
                blob(at + 2, usize::from(u16::from_le_bytes([len[0], len[1]])))
            }
            TYPE_BLOB_32 => blob(
                at + 4,
                usize::try_from(u32_at(data, at)?).map_err(|_| invalid("blob"))?,
            ),
            TYPE_ARRAY | TYPE_OBJECT => {
                let count = u32_at(data, at)?;
                if count == 0 || count > MAX_SLOTS {
                    return Err(invalid("slot count"));
                }
                (1..count)
                    .map(|slot| u32_at(data, at + 4 * usize::try_from(slot).unwrap_or(usize::MAX)))
                    .collect::<Result<_, _>>()
                    .map(Value::Slots)
            }
            _ => Err(invalid("unknown value type")),
        }
    }

    fn slots(&self, raw: u32) -> Result<Vec<u32>, Error> {
        match self.value(raw)? {
            Value::Slots(slots) => Ok(slots),
            Value::Null => Ok(Vec::new()),
            _ => Err(invalid("expected an object or array")),
        }
    }

    /// Slot `index` (1-based) of an object's `slots`, or null.
    fn field(&self, slots: &[u32], index: usize) -> Result<Value<'a>, Error> {
        slots
            .get(index - 1)
            .map_or(Ok(Value::Null), |&raw| self.value(raw))
    }

    fn text(&self, slots: &[u32], index: usize) -> Result<Option<String>, Error> {
        Ok(match self.field(slots, index)? {
            Value::Blob(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
    }

    fn int(&self, slots: &[u32], index: usize) -> Result<Option<u64>, Error> {
        Ok(match self.field(slots, index)? {
            Value::Int(n) => Some(n),
            _ => None,
        })
    }

    /// The names of a dependency array's items, conflicts left out.
    fn names(&self, slots: &[u32], index: usize) -> Result<Vec<String>, Error> {
        let Some(&raw) = slots.get(index - 1) else {
            return Ok(Vec::new());
        };
        let mut names = Vec::new();
        for item in self.slots(raw)? {
            let dep = self.slots(item)?;
            if self.int(&dep, DEP_MATCH)?.unwrap_or(0) & MATCH_CONFLICT != 0 {
                continue;
            }
            if let Some(name) = self.text(&dep, DEP_NAME)?.filter(|name| !name.is_empty()) {
                names.push(name);
            }
        }
        Ok(names)
    }
}

/// Reads a `packages.adb`. A package whose file name the mirror can't keep is left out.
pub fn parse(bytes: &[u8]) -> Result<Index, Error> {
    let stream = inflate(bytes)?;
    let data = first_block(&stream, SCHEMA_INDEX)?;
    let db = Db { data };
    let root = u32_at(data, 4)?;
    let index = db.slots(root)?;
    let spec = db
        .text(&index, NDX_PKGNAME_SPEC)?
        .unwrap_or_else(|| DEFAULT_SPEC.to_owned());
    let packages = match index.get(NDX_PACKAGES - 1) {
        Some(&raw) => db.slots(raw)?,
        None => Vec::new(),
    };
    let mut entries = Vec::with_capacity(packages.len());
    for raw in packages {
        let pkg = db.slots(raw)?;
        let (Some(name), Some(version)) = (db.text(&pkg, PI_NAME)?, db.text(&pkg, PI_VERSION)?)
        else {
            continue;
        };
        let arch = db.text(&pkg, PI_ARCH)?.unwrap_or_default();
        let Some(file) = file_name(&spec, &name, &version, &arch) else {
            continue;
        };
        let check = match db.field(&pkg, PI_HASHES)? {
            Value::Blob(hash) => <[u8; 32]>::try_from(hash).ok().map(Check::ApkControl),
            _ => None,
        };
        entries.push(Entry {
            size: db.int(&pkg, PI_FILE_SIZE)?.unwrap_or(0),
            check,
            depends: db
                .names(&pkg, PI_DEPENDS)?
                .into_iter()
                .map(|name| vec![name])
                .collect(),
            provides: db.names(&pkg, PI_PROVIDES)?,
            name,
            version,
            file,
        });
    }
    Ok(Index { entries })
}

/// A package's file name by the index's `pkgname_spec`; `None` when the spec uses another
/// variable or names a file outside the feed's directory.
fn file_name(spec: &str, name: &str, version: &str, arch: &str) -> Option<String> {
    let file = spec
        .replace("${name}", name)
        .replace("${version}", version)
        .replace("${arch}", arch);
    (!file.contains('$') && valid_segment(&file)).then_some(file)
}
