//! One file out of a gzipped tarball (POSIX ustar or GNU, as CI's `tar` writes them; D-123):
//! headers checked by their checksum, pax and long-name records skipped, nothing but the member
//! asked for written.

use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
};

use flate2::read::GzDecoder;

const BLOCK: usize = 512;

/// Copies `member` (a regular file) of the gzipped tarball `archive` to `out` (created, mode
/// 0755), at most `max` bytes; `false` when the archive has no such member.
pub(crate) fn extract(archive: &Path, member: &str, out: &Path, max: u64) -> io::Result<bool> {
    let mut tar = GzDecoder::new(BufReader::new(File::open(archive)?));
    let mut header = [0_u8; BLOCK];
    loop {
        if !read_block(&mut tar, &mut header)? || header.iter().all(|b| *b == 0) {
            return Ok(false);
        }
        if !checksum_holds(&header) {
            return Err(invalid("a tar header's checksum doesn't hold"));
        }
        let size = octal(&header[124..136]).ok_or_else(|| invalid("a tar header's size"))?;
        let kind = header[156];
        let name = name(&header).ok_or_else(|| invalid("a tar member's name"))?;
        if name == member && (kind == b'0' || kind == 0) {
            if size > max {
                return Err(invalid("the binary is too large"));
            }
            let mut file = create_executable(out)?;
            let copied = io::copy(&mut (&mut tar).take(size), &mut file)?;
            if copied != size {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            file.sync_all()?;
            return Ok(true);
        }
        let padded = size.div_ceil(BLOCK as u64) * BLOCK as u64;
        let skipped = io::copy(&mut (&mut tar).take(padded), &mut io::sink())?;
        if skipped != padded {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    }
}

fn create_executable(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o755)
        .open(path)
}

/// A whole block; `false` at the end of the stream.
fn read_block(reader: &mut impl Read, block: &mut [u8; BLOCK]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < BLOCK {
        match reader.read(&mut block[filled..])? {
            0 if filled == 0 => return Ok(false),
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => filled += n,
        }
    }
    Ok(true)
}

/// The header's checksum: the sum of its bytes with the checksum field read as spaces.
fn checksum_holds(header: &[u8; BLOCK]) -> bool {
    let sum: u64 = header
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if (148..156).contains(&i) {
                32
            } else {
                u64::from(*b)
            }
        })
        .sum();
    octal(&header[148..156]) == Some(sum)
}

/// An octal field, NUL- or space-terminated.
fn octal(field: &[u8]) -> Option<u64> {
    let digits: &[u8] = field
        .split(|b| *b == 0 || *b == b' ')
        .find(|part| !part.is_empty())?;
    let text = std::str::from_utf8(digits).ok()?;
    u64::from_str_radix(text, 8).ok()
}

/// The member's path: ustar's prefix and name joined by `/`.
fn name(header: &[u8; BLOCK]) -> Option<String> {
    let field = |bytes: &[u8]| -> Option<String> {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[..end]).ok().map(str::to_owned)
    };
    let name = field(&header[..100])?;
    let ustar = &header[257..263] == b"ustar\0";
    let prefix = if ustar {
        field(&header[345..500])?
    } else {
        String::new()
    };
    Some(if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    })
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("bad tarball: {what}"))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::extract;

    fn write(path: &std::path::Path, files: &[(&str, &[u8])]) {
        std::fs::write(path, crate::testing::tarball(files)).unwrap();
    }

    #[test]
    fn the_member_asked_for_is_extracted_executable() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        let big = vec![7_u8; 1300];
        write(
            &archive,
            &[
                ("d/README.md", b"read me"),
                ("d/uniwrt", &big),
                ("d/LICENSE", b"x"),
            ],
        );
        let out = dir.path().join("bin");
        assert!(extract(&archive, "d/uniwrt", &out, 1 << 20).unwrap());
        assert_eq!(std::fs::read(&out).unwrap(), big);
        let mode = std::fs::metadata(&out).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(!extract(&archive, "d/other", &out, 1 << 20).unwrap());
        assert!(extract(&archive, "d/uniwrt", &out, 1000).is_err());
    }

    #[test]
    fn a_corrupt_header_or_a_cut_archive_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        write(&archive, &[("d/uniwrt", b"binary")]);
        // Inflate, alter the name (the checksum no longer holds), deflate again.
        let mut tar = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(std::fs::File::open(&archive).unwrap()),
            &mut tar,
        )
        .unwrap();
        let mut altered = tar.clone();
        altered[0] = b'e';
        let gz = |bytes: &[u8], path: &std::path::Path| {
            let mut enc = flate2::write::GzEncoder::new(
                std::fs::File::create(path).unwrap(),
                flate2::Compression::fast(),
            );
            std::io::Write::write_all(&mut enc, bytes).unwrap();
            enc.finish().unwrap();
        };
        gz(&altered, &archive);
        let out = dir.path().join("bin");
        assert!(extract(&archive, "e/uniwrt", &out, 1 << 20).is_err());
        gz(&tar[..512 + 3], &archive);
        assert!(extract(&archive, "d/uniwrt", &out, 1 << 20).is_err());
    }
}
