//! Writing ADB files, the reverse of [`super::parse`]: a feed's index (`indx`) and a package
//! whose control block is a `pckg` database naming it. apk-tools would refuse both (they carry
//! no signature block); they exist for demo mode's made-up apk feeds and for tests, so the
//! mirror's apk path runs end to end without internet.

use std::io::Write as _;

use flate2::{Compression, write::DeflateEncoder};
use sha2::{Digest as _, Sha256};

use super::{
    DEP_NAME, NDX_PACKAGES, PI_DEPENDS, PI_FILE_SIZE, PI_HASHES, PI_NAME, PI_PROVIDES, PI_VERSION,
    SCHEMA_INDEX, SCHEMA_PACKAGE, TYPE_ARRAY, TYPE_BLOB_8, TYPE_BLOB_16, TYPE_BLOB_32, TYPE_INT,
    TYPE_INT_64, TYPE_OBJECT, VALUE_MASK, invalid,
};
use crate::{
    Error,
    index::{Check, Index},
};

/// A database block's payload being written: the header (versions, then the root value), then
/// values, each placed where the values that point at it can reach (offsets under 2²⁸).
struct Db {
    data: Vec<u8>,
}

impl Db {
    fn new() -> Self {
        Self { data: vec![0; 8] }
    }

    fn align(&mut self, to: usize) {
        while !self.data.len().is_multiple_of(to) {
            self.data.push(0);
        }
    }

    /// The offset of what is written next, as a value's number.
    fn at(&self) -> Result<u32, Error> {
        u32::try_from(self.data.len())
            .ok()
            .filter(|&at| at <= VALUE_MASK)
            .ok_or_else(|| invalid("the database outgrows its offsets"))
    }

    fn blob(&mut self, bytes: &[u8]) -> Result<u32, Error> {
        let kind = if let Ok(len) = u8::try_from(bytes.len()) {
            let at = self.at()?;
            self.data.push(len);
            TYPE_BLOB_8 | at
        } else if let Ok(len) = u16::try_from(bytes.len()) {
            self.align(2);
            let at = self.at()?;
            self.data.extend_from_slice(&len.to_le_bytes());
            TYPE_BLOB_16 | at
        } else {
            let len = u32::try_from(bytes.len()).map_err(|_| invalid("blob size"))?;
            self.align(4);
            let at = self.at()?;
            self.data.extend_from_slice(&len.to_le_bytes());
            TYPE_BLOB_32 | at
        };
        self.data.extend_from_slice(bytes);
        Ok(kind)
    }

    fn int(&mut self, n: u64) -> Result<u32, Error> {
        if let Ok(small) = u32::try_from(n)
            && small <= VALUE_MASK
        {
            return Ok(TYPE_INT | small);
        }
        self.align(8);
        let at = self.at()?;
        self.data.extend_from_slice(&n.to_le_bytes());
        Ok(TYPE_INT_64 | at)
    }

    /// An object (`TYPE_OBJECT`) or array (`TYPE_ARRAY`) of `slots`, slot 1 first; an object's
    /// trailing nulls are left out, as apk does.
    fn slots(&mut self, kind: u32, slots: &[u32]) -> Result<u32, Error> {
        let used = if kind == TYPE_OBJECT {
            slots
                .iter()
                .rposition(|&raw| raw != 0)
                .map_or(0, |last| last + 1)
        } else {
            slots.len()
        };
        let count = u32::try_from(used + 1).map_err(|_| invalid("slot count"))?;
        self.align(4);
        let at = self.at()?;
        self.data.extend_from_slice(&count.to_le_bytes());
        for raw in &slots[..used] {
            self.data.extend_from_slice(&raw.to_le_bytes());
        }
        Ok(kind | at)
    }

    /// The payload, `root` set as the database's root.
    fn finish(mut self, root: u32) -> Vec<u8> {
        self.data[4..8].copy_from_slice(&root.to_le_bytes());
        self.data
    }

    /// An array of dependencies, each by name.
    fn names(&mut self, names: impl IntoIterator<Item = impl AsRef<str>>) -> Result<u32, Error> {
        let mut items = Vec::new();
        for name in names {
            let mut dep = [0; DEP_NAME];
            dep[DEP_NAME - 1] = self.blob(name.as_ref().as_bytes())?;
            items.push(self.slots(TYPE_OBJECT, &dep)?);
        }
        if items.is_empty() {
            return Ok(0);
        }
        self.slots(TYPE_ARRAY, &items)
    }
}

/// `ADBd`: the deflated stream of `schema` and `blocks` (kind, payload), each padded to 8.
fn file(schema: [u8; 4], blocks: &[(u32, &[u8])]) -> Result<Vec<u8>, Error> {
    let mut stream = b"ADB.".to_vec();
    stream.extend_from_slice(&schema);
    for &(kind, payload) in blocks {
        let size = u32::try_from(payload.len() + 4)
            .ok()
            .filter(|&size| size <= 0x3fff_ffff)
            .ok_or_else(|| invalid("block size"))?;
        stream.extend_from_slice(&(kind << 30 | size).to_le_bytes());
        stream.extend_from_slice(payload);
        while !stream.len().is_multiple_of(8) {
            stream.push(0);
        }
    }
    let mut out = b"ADBd".to_vec();
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(&stream)
        .map_err(|err| invalid(format!("deflate: {err}")))?;
    out.extend(
        encoder
            .finish()
            .map_err(|err| invalid(format!("deflate: {err}")))?,
    );
    Ok(out)
}

/// A `packages.adb` listing `index`'s entries by apk's default file name
/// (`${name}-${version}.apk`, so an entry's `file` isn't written): each with its size, its
/// control block's hash (an opkg `Check` is left out), its dependencies (an item's first
/// alternative: apk has none) and the names it provides.
pub fn write(index: &Index) -> Result<Vec<u8>, Error> {
    let mut db = Db::new();
    let mut packages = Vec::with_capacity(index.entries.len());
    for entry in &index.entries {
        let mut pkg = [0; PI_PROVIDES];
        pkg[PI_NAME - 1] = db.blob(entry.name.as_bytes())?;
        pkg[PI_VERSION - 1] = db.blob(entry.version.as_bytes())?;
        if let Some(Check::ApkControl(hash)) = entry.check {
            pkg[PI_HASHES - 1] = db.blob(&hash)?;
        }
        pkg[PI_FILE_SIZE - 1] = db.int(entry.size)?;
        pkg[PI_DEPENDS - 1] = db.names(entry.depends.iter().filter_map(|alts| alts.first()))?;
        pkg[PI_PROVIDES - 1] = db.names(&entry.provides)?;
        packages.push(db.slots(TYPE_OBJECT, &pkg)?);
    }
    let mut root = [0; NDX_PACKAGES];
    if !packages.is_empty() {
        root[NDX_PACKAGES - 1] = db.slots(TYPE_ARRAY, &packages)?;
    }
    let root = db.slots(TYPE_OBJECT, &root)?;
    file(SCHEMA_INDEX, &[(0, &db.finish(root))])
}

/// A package file: its control block (a `pckg` database of `name` and `version`), then
/// `data` as one data block; with the SHA-256 of the control block, which an index records.
pub fn package(name: &str, version: &str, data: &[u8]) -> Result<(Vec<u8>, [u8; 32]), Error> {
    let mut db = Db::new();
    let mut info = [0; PI_VERSION];
    info[PI_NAME - 1] = db.blob(name.as_bytes())?;
    info[PI_VERSION - 1] = db.blob(version.as_bytes())?;
    let info = db.slots(TYPE_OBJECT, &info)?;
    // A package's root: its info first.
    let root = db.slots(TYPE_OBJECT, &[info])?;
    let control = db.finish(root);
    let hash = Sha256::digest(&control).into();
    Ok((file(SCHEMA_PACKAGE, &[(0, &control), (2, data)])?, hash))
}
