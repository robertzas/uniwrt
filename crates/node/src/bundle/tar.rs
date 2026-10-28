//! A minimal ustar writer: directories and regular files with a fixed owner, mode and time, so
//! the same files always make the same bytes. BusyBox `tar xf -` reads it on the node.

const BLOCK: usize = 512;
/// The longest path a ustar header holds without the prefix field.
pub(super) const MAX_PATH: usize = 99;

#[derive(Default)]
pub(super) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(super) fn dir(&mut self, path: &str) {
        self.entry(&format!("{path}/"), b'5', 0o700, &[]);
    }

    pub(super) fn file(&mut self, path: &str, content: &[u8]) {
        self.entry(path, b'0', 0o600, content);
    }

    /// The archive, closed by two empty blocks.
    pub(super) fn finish(mut self) -> Vec<u8> {
        self.buf.resize(self.buf.len() + 2 * BLOCK, 0);
        self.buf
    }

    fn entry(&mut self, path: &str, kind: u8, mode: u64, content: &[u8]) {
        debug_assert!(
            path.len() <= MAX_PATH,
            "{path} is too long for a ustar header"
        );
        let mut header = [0; BLOCK];
        let name = &path.as_bytes()[..path.len().min(MAX_PATH)];
        header[..name.len()].copy_from_slice(name);
        octal(&mut header[100..108], mode);
        octal(&mut header[108..116], 0); // uid
        octal(&mut header[116..124], 0); // gid
        octal(&mut header[124..136], content.len() as u64);
        octal(&mut header[136..148], 0); // mtime
        header[156] = kind;
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        // The checksum counts its own field as eight spaces.
        header[148..156].fill(b' ');
        let sum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        self.buf.extend_from_slice(&header);
        self.buf.extend_from_slice(content);
        self.buf.resize(self.buf.len().next_multiple_of(BLOCK), 0);
    }
}

/// Zero-padded octal digits filling the field but its last byte, which stays NUL.
fn octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    field[..width].copy_from_slice(format!("{value:0width$o}").as_bytes());
}

#[cfg(test)]
mod tests {
    use std::{io::Write, process::Command};

    use super::Writer;

    /// The host's tar (GNU or BusyBox) lists what was written, with sizes and modes.
    #[test]
    fn tar_reads_the_archive() {
        let mut writer = Writer::default();
        writer.dir("collect");
        writer.file("collect/board.uc", b"// board\n");
        writer.file("empty", b"");
        writer.file("big", &[b'x'; 1000]);
        let archive = writer.finish();
        assert_eq!(archive.len() % 512, 0);

        let mut tar = Command::new("tar")
            .args(["-tvf", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        tar.stdin.take().unwrap().write_all(&archive).unwrap();
        let output = tar.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let listing = String::from_utf8(output.stdout).unwrap();
        let lines: Vec<_> = listing.lines().collect();
        assert_eq!(lines.len(), 4, "{listing}");
        assert!(lines[0].starts_with("drwx------") && lines[0].ends_with("collect/"));
        assert!(lines[1].starts_with("-rw-------") && lines[1].ends_with("collect/board.uc"));
        assert!(lines[3].contains(" 1000 ") && lines[3].ends_with("big"));
    }
}
