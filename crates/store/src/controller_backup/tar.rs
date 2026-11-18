//! The few ustar entries a controller backup holds, written and read as streams: regular files
//! with short names, mode 0600, so `tar xz` unpacks them into a data directory as they are.

use std::io::{self, Read, Write};

const BLOCK: usize = 512;
/// ustar's size field: 11 octal digits.
const MAX_SIZE: u64 = 0o77_777_777_777;

/// Writes `len` bytes from `data` as the regular file `name`.
pub(super) fn write_file(
    out: &mut impl Write,
    name: &str,
    len: u64,
    mtime: i64,
    data: &mut impl Read,
) -> io::Result<()> {
    if name.len() >= 100 || len > MAX_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "entry too large for ustar",
        ));
    }
    let mut header = [0u8; BLOCK];
    header[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut header[100..108], 0o600);
    octal(&mut header[108..116], 0);
    octal(&mut header[116..124], 0);
    octal(&mut header[124..136], len);
    octal(&mut header[136..148], u64::try_from(mtime).unwrap_or(0));
    header[156] = b'0';
    header[257..265].copy_from_slice(b"ustar\x0000");
    header[148..156].fill(b' ');
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    octal(&mut header[148..155], u64::from(sum));
    out.write_all(&header)?;
    let copied = io::copy(&mut data.take(len), out)?;
    if copied != len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("{name} shrank while it was read"),
        ));
    }
    out.write_all(&[0; BLOCK][..padding(len)])
}

/// The two zero blocks that end an archive.
pub(super) fn finish(out: &mut impl Write) -> io::Result<()> {
    out.write_all(&[0; 2 * BLOCK])
}

/// The next entry's name and size, or `None` at the archive's end; its data must be read with
/// [`read_data`] before the next.
pub(super) fn next_entry(input: &mut impl Read) -> io::Result<Option<(String, u64)>> {
    let mut header = [0u8; BLOCK];
    input.read_exact(&mut header)?;
    if header.iter().all(|&b| b == 0) {
        return Ok(None);
    }
    let bad =
        |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("bad tar entry: {why}"));
    let stored = parse_octal(&header[148..156]).ok_or_else(|| bad("checksum"))?;
    header[148..156].fill(b' ');
    if stored != header.iter().map(|&b| u64::from(b)).sum::<u64>() {
        return Err(bad("checksum"));
    }
    if !matches!(header[156], b'0' | 0) {
        return Err(bad("not a regular file"));
    }
    let name = header[..100].split(|&b| b == 0).next().unwrap_or_default();
    let name = std::str::from_utf8(name)
        .map_err(|_| bad("name"))?
        .to_owned();
    let size = parse_octal(&header[124..136]).ok_or_else(|| bad("size"))?;
    Ok(Some((name, size)))
}

/// Copies an entry's `size` bytes into `out` and skips its padding.
pub(super) fn read_data(input: &mut impl Read, size: u64, out: &mut impl Write) -> io::Result<()> {
    if io::copy(&mut input.take(size), out)? != size {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    input.read_exact(&mut [0; BLOCK][..padding(size)])
}

fn padding(len: u64) -> usize {
    // Below 512: fits.
    usize::try_from(len.next_multiple_of(BLOCK as u64) - len).unwrap_or(0)
}

/// `value` in octal, zero-padded, NUL-terminated, filling `field`.
fn octal(field: &mut [u8], value: u64) {
    let digits = field.len() - 1;
    let text = format!("{value:0digits$o}");
    field[..digits].copy_from_slice(&text.as_bytes()[text.len() - digits..]);
    field[digits] = 0;
}

fn parse_octal(field: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(field).ok()?;
    let text = text.trim_matches(|c: char| c == ' ' || c == '\0');
    u64::from_str_radix(text, 8).ok()
}

#[cfg(test)]
mod tests {
    use super::{finish, next_entry, read_data, write_file};

    #[test]
    fn entries_round_trip_in_ustar() {
        let mut tar = Vec::new();
        for (name, data) in [
            ("a.json", &b"{}"[..]),
            ("empty", b""),
            ("big", &[7; 1_000][..]),
        ] {
            write_file(
                &mut tar,
                name,
                data.len() as u64,
                1_700_000_000,
                &mut &data[..],
            )
            .unwrap();
        }
        finish(&mut tar).unwrap();
        assert_eq!(tar.len(), 512 * (1 + 1 + 1 + 1 + 2 + 2));
        assert_eq!(&tar[257..263], b"ustar\0");
        let mut input = tar.as_slice();
        let mut seen = Vec::new();
        while let Some((name, size)) = next_entry(&mut input).unwrap() {
            let mut data = Vec::new();
            read_data(&mut input, size, &mut data).unwrap();
            seen.push((name, data));
        }
        assert_eq!(seen[0], ("a.json".to_owned(), b"{}".to_vec()));
        assert_eq!(seen[1].1, b"");
        assert_eq!(seen[2].1, vec![7; 1_000]);
        // An altered header fails its checksum.
        let mut altered = tar.clone();
        altered[0] = b'b';
        assert!(next_entry(&mut altered.as_slice()).is_err());
        // A source shorter than said fails rather than writing a short entry.
        assert!(write_file(&mut Vec::new(), "x", 5, 0, &mut &b"abc"[..]).is_err());
    }
}
