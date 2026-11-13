//! What an uploaded floor plan is, from its header alone: PNG or still WebP, and its size. The
//! browser re-encodes every plan (D-117), so anything else is refused rather than decoded.

use uniwrt_model::floor::ImageKind;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The image's kind and size in pixels, or None when it is neither a PNG nor a still WebP.
pub(super) fn sniff(data: &[u8]) -> Option<(ImageKind, u32, u32)> {
    if data.starts_with(PNG) {
        return png(data).map(|(width, height)| (ImageKind::Png, width, height));
    }
    webp(data).map(|(width, height)| (ImageKind::Webp, width, height))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn le32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn le24(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 3)?;
    Some(u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16)
}

/// The first chunk is IHDR, with the width and height.
fn png(data: &[u8]) -> Option<(u32, u32)> {
    (data.get(12..16)? == b"IHDR").then_some(())?;
    Some((be32(data, 16)?, be32(data, 20)?))
}

/// RIFF's size must fit the data; the first chunk says the encoding (RFC 9649).
fn webp(data: &[u8]) -> Option<(u32, u32)> {
    if data.get(0..4)? != b"RIFF" || data.get(8..12)? != b"WEBP" {
        return None;
    }
    let riff = usize::try_from(le32(data, 4)?).ok()?;
    if riff.checked_add(8)? > data.len() {
        return None;
    }
    match data.get(12..16)? {
        b"VP8 " => {
            // A key frame's start code, then 14-bit width and height.
            (data.get(23..26)? == [0x9d, 0x01, 0x2a]).then_some(())?;
            let width = u32::from(u16::from_le_bytes(data.get(26..28)?.try_into().ok()?)) & 0x3fff;
            let height = u32::from(u16::from_le_bytes(data.get(28..30)?.try_into().ok()?)) & 0x3fff;
            Some((width, height))
        }
        b"VP8L" => {
            (*data.get(20)? == 0x2f).then_some(())?;
            let bits = le32(data, 21)?;
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => {
            const ANIMATION: u8 = 0x02;
            (data.get(20)? & ANIMATION == 0).then_some(())?;
            Some((le24(data, 24)? + 1, le24(data, 27)? + 1))
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use uniwrt_model::floor::ImageKind;

    use super::sniff;

    /// A PNG's signature and IHDR, which is all the sniffing reads.
    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut data = super::PNG.to_vec();
        data.extend_from_slice(&13u32.to_be_bytes());
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        data
    }

    fn riff(chunk: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&(u32::try_from(payload.len()).unwrap() + 12).to_le_bytes());
        data.extend_from_slice(b"WEBP");
        data.extend_from_slice(&chunk);
        data.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        data.extend_from_slice(payload);
        data
    }

    /// A lossless WebP's header.
    pub(crate) fn webp(width: u32, height: u32) -> Vec<u8> {
        let bits = (width - 1) | (height - 1) << 14;
        let mut payload = vec![0x2f];
        payload.extend_from_slice(&bits.to_le_bytes());
        riff(*b"VP8L", &payload)
    }

    #[test]
    fn png_and_webp_headers_give_their_size() {
        assert_eq!(sniff(&png(800, 600)), Some((ImageKind::Png, 800, 600)));
        assert_eq!(sniff(&webp(4096, 3)), Some((ImageKind::Webp, 4096, 3)));
        // Lossy: a key frame's tag, the start code, then the size.
        let lossy = riff(
            *b"VP8 ",
            &[0, 0, 0, 0x9d, 0x01, 0x2a, 0x20, 0x03, 0x58, 0x02],
        );
        assert_eq!(sniff(&lossy), Some((ImageKind::Webp, 800, 600)));
        // Extended: flags, three reserved bytes, then the canvas size less one.
        let mut extended = vec![0x10, 0, 0, 0];
        extended.extend_from_slice(&[0x1f, 0x03, 0, 0x57, 0x02, 0]);
        assert_eq!(
            sniff(&riff(*b"VP8X", &extended)),
            Some((ImageKind::Webp, 800, 600))
        );
    }

    #[test]
    fn anything_else_is_refused() {
        let mut animated = vec![0x02, 0, 0, 0];
        animated.extend_from_slice(&[0x1f, 0x03, 0, 0x57, 0x02, 0]);
        assert_eq!(sniff(&riff(*b"VP8X", &animated)), None);
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(sniff(b"\xff\xd8\xff\xe0JFIF"), None);
        assert_eq!(sniff(&png(1, 1)[..20]), None);
        let mut short = webp(10, 10);
        short.truncate(short.len() - 1);
        assert_eq!(sniff(&short), None);
        assert_eq!(sniff(&[]), None);
    }
}
