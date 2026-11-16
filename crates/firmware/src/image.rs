//! What an uploaded image is (pure; M7.1a2, D-122): OpenWrt's `fwtool` appends chunks to a
//! sysupgrade image, each ended by a 16-byte trailer (`FWx0`, a CRC-32, its type, its size
//! with the trailer, big-endian): the metadata (type 1: an 8-byte header, then JSON with the
//! boards it fits and its release) and a signature (type 0). x86 images carry no metadata: a
//! gzipped disk image is taken as one, its target read from its name (`…-x86-64-…`).

use serde::Deserialize;

use crate::Error;

/// How much of an image's end holds its metadata: the chunks are small.
pub(crate) const TAIL: usize = 64 << 10;

const TRAILER: usize = 16;
const MAGIC: &[u8; 4] = b"FWx0";
const METADATA: u8 = 1;
const HEADER: usize = 8;

/// An image's identity, from its metadata or its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inspected {
    pub(crate) target: String,
    /// Empty for an x86 image.
    pub(crate) boards: Vec<String>,
    pub(crate) distribution: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) revision: Option<String>,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    supported_devices: Vec<String>,
    #[serde(default)]
    version: Option<Version>,
}

#[derive(Deserialize)]
struct Version {
    #[serde(default)]
    dist: Option<String>,
    #[serde(default, rename = "version")]
    number: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

/// What the image named `name` is, from its first bytes and its last [`TAIL`] ones.
pub(crate) fn inspect(name: &str, head: &[u8], tail: &[u8]) -> Result<Inspected, Error> {
    if let Some(json) = metadata(tail) {
        let raw: Metadata = serde_json::from_slice(json).map_err(|err| {
            Error::Invalid(format!("The image's metadata is not as expected: {err}"))
        })?;
        let version = raw.version.unwrap_or(Version {
            dist: None,
            number: None,
            revision: None,
            target: None,
        });
        let target = version
            .target
            .filter(|target| crate::catalog::valid_target(target))
            .ok_or_else(|| Error::Invalid("The image's metadata names no target".to_owned()))?;
        let boards: Vec<String> = raw
            .supported_devices
            .into_iter()
            .filter(|board| valid_board(board))
            .collect();
        if boards.is_empty() {
            return Err(Error::Invalid(
                "The image's metadata names no board it fits".to_owned(),
            ));
        }
        return Ok(Inspected {
            target,
            boards,
            distribution: version.dist.filter(|text| printable(text)),
            version: version.number.filter(|text| printable(text)),
            revision: version.revision.filter(|text| printable(text)),
        });
    }
    if !head.starts_with(&[0x1f, 0x8b]) {
        return Err(Error::Invalid(
            "This isn't an OpenWrt sysupgrade image: it carries no metadata naming the boards it \
             fits (only x86's gzipped images may lack it)"
                .to_owned(),
        ));
    }
    let target = x86_target(name).ok_or_else(|| {
        Error::Invalid(
            "An x86 image without metadata needs its target in its name, as OpenWrt names it \
             (…-x86-64-…, …-x86-generic-…, …-x86-legacy-…, …-x86-geode-…)"
                .to_owned(),
        )
    })?;
    Ok(Inspected {
        target: target.to_owned(),
        boards: Vec::new(),
        distribution: None,
        version: None,
        revision: None,
    })
}

/// The metadata's JSON: the chunks are walked back from the end until a metadata one.
fn metadata(tail: &[u8]) -> Option<&[u8]> {
    let mut end = tail.len();
    while end >= TRAILER {
        let trailer = &tail[end - TRAILER..end];
        if &trailer[..4] != MAGIC {
            return None;
        }
        let kind = trailer[8];
        let size = u32::from_be_bytes([trailer[12], trailer[13], trailer[14], trailer[15]]);
        let size = usize::try_from(size).ok()?;
        if size < TRAILER || size > end {
            return None;
        }
        if kind == METADATA {
            let chunk = &tail[end - size..end - TRAILER];
            return chunk.get(HEADER..);
        }
        end -= size;
    }
    None
}

/// The x86 subtarget an official image's name tells.
fn x86_target(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    [
        ("-x86-64-", "x86/64"),
        ("-x86-generic-", "x86/generic"),
        ("-x86-legacy-", "x86/legacy"),
        ("-x86-geode-", "x86/geode"),
    ]
    .into_iter()
    .find_map(|(part, target)| name.contains(part).then_some(target))
}

/// A board name as OpenWrt writes it (`glinet,gl-mt6000`, `archer-c7-v5`).
fn valid_board(board: &str) -> bool {
    !board.is_empty()
        && board.len() <= 128
        && board
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b',' | b'-' | b'_' | b'.' | b'+'))
}

fn printable(text: &str) -> bool {
    !text.is_empty() && text.len() <= 128 && text.chars().all(|c| !c.is_control())
}

/// A file name kept for people: its last path part, printable, at most 200 bytes.
pub(crate) fn file_name(name: &str) -> Result<String, Error> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if base.is_empty() || base.len() > 200 || base.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "The image needs a file name (at most 200 bytes)".to_owned(),
        ));
    }
    Ok(base.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chunk as fwtool appends it.
    fn chunk(kind: u8, data: &[u8]) -> Vec<u8> {
        let mut out = data.to_vec();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&0x1234_5678_u32.to_be_bytes());
        out.extend_from_slice(&[kind, 0, 0, 0]);
        let size = u32::try_from(data.len() + TRAILER).unwrap();
        out.extend_from_slice(&size.to_be_bytes());
        out
    }

    /// The end of OpenWrt 24.10.4's GL-MT6000 sysupgrade image, as the download server has it.
    fn mt6000() -> Vec<u8> {
        let json = br#"{  "metadata_version": "1.1", "compat_version": "1.0",   "supported_devices":["glinet,gl-mt6000"], "version": { "release": "24.10.4", "date": "20251021190330", "dist": "OpenWrt", "version": "24.10.4", "revision": "r28959-29397011cc", "target": "mediatek/filogic", "board": "glinet_gl-mt6000" } }"#;
        let mut meta = vec![0; HEADER];
        meta.extend_from_slice(json);
        let mut image = vec![0xff; 64];
        image.extend(chunk(METADATA, &meta));
        image.extend(chunk(0, b"# fake certificate"));
        image
    }

    #[test]
    fn the_metadata_names_the_boards_and_the_release() {
        let found = inspect("openwrt.bin", &[0x27, 0x05], &mt6000()).unwrap();
        assert_eq!(
            found,
            Inspected {
                target: "mediatek/filogic".to_owned(),
                boards: vec!["glinet,gl-mt6000".to_owned()],
                distribution: Some("OpenWrt".to_owned()),
                version: Some("24.10.4".to_owned()),
                revision: Some("r28959-29397011cc".to_owned()),
            }
        );
    }

    #[test]
    fn an_x86_image_is_known_by_its_name() {
        let mut image = vec![0x1f, 0x8b, 8, 0];
        image.extend(chunk(0, b"# fake certificate"));
        let found = inspect(
            "openwrt-24.10.4-x86-64-generic-squashfs-combined-efi.img.gz",
            &image,
            &image,
        )
        .unwrap();
        assert_eq!(found.target, "x86/64");
        assert_eq!(found.boards, [] as [String; 0]);
        assert!(found.version.is_none());
        let unnamed = inspect("firmware.img.gz", &image, &image);
        assert!(matches!(unnamed, Err(Error::Invalid(why)) if why.contains("x86-64")));
    }

    #[test]
    fn an_image_without_metadata_or_boards_is_refused() {
        let plain = vec![0x27, 0x05, 0x19, 0x56, 0, 0];
        assert!(inspect("x86-64-image.bin", &plain, &plain).is_err());
        let mut meta = vec![0; HEADER];
        meta.extend_from_slice(br#"{"supported_devices":[],"version":{"target":"ath79/generic"}}"#);
        let image = chunk(METADATA, &meta);
        assert!(
            matches!(inspect("a.bin", &image, &image), Err(Error::Invalid(why)) if why.contains("no board"))
        );
        let mut meta = vec![0; HEADER];
        meta.extend_from_slice(br#"{"supported_devices":["a,b"],"version":{"target":"../x"}}"#);
        let image = chunk(METADATA, &meta);
        assert!(inspect("a.bin", &image, &image).is_err());
    }

    #[test]
    fn a_trailer_whose_size_runs_past_the_tail_ends_the_walk() {
        let mut image = chunk(0, b"sig");
        let len = image.len();
        image[len - 4..].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(metadata(&image), None);
    }

    #[test]
    fn file_names_keep_their_last_part() {
        assert_eq!(file_name("C:\\dl\\openwrt.bin").unwrap(), "openwrt.bin");
        assert_eq!(file_name("a/b/x.img.gz").unwrap(), "x.img.gz");
        assert!(file_name("dir/").is_err());
        assert!(file_name("a\nb").is_err());
    }
}
