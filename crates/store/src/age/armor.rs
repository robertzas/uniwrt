//! age's ASCII armor (C2SP's strict PEM): the file as standard Base64 with padding, 64 columns
//! to a line (the last shorter or equal), between `-----BEGIN AGE ENCRYPTED FILE-----` and
//! `-----END AGE ENCRYPTED FILE-----`, so that an age file fits in a JSON string or a text
//! file and `age -d` still opens it (a site export's secrets, D-130). Reading accepts LF or
//! CRLF and whitespace around the whole, nothing else.

use std::io::{Read as _, Write as _};

use data_encoding::BASE64;
use zeroize::Zeroizing;

use super::{COLUMNS, Decryptor, Encryptor, LOG_N};
use crate::{Error, Result};

const BEGIN: &str = "-----BEGIN AGE ENCRYPTED FILE-----";
const END: &str = "-----END AGE ENCRYPTED FILE-----";
/// The longest armored file read: far more than a site's secrets need.
pub const MAX_ARMORED: usize = 1 << 20;

/// `plain` encrypted for `passphrase` at age's default work factor, armored.
pub fn seal(plain: &[u8], passphrase: &[u8]) -> Result<String> {
    seal_with(plain, passphrase, LOG_N)
}

/// The same at work factor `log_n` (cheap ones for tests).
pub fn seal_with(plain: &[u8], passphrase: &[u8], log_n: u8) -> Result<String> {
    let mut age = Encryptor::new(Vec::new(), passphrase, log_n)?;
    age.write_all(plain).map_err(|err| io_error(&err))?;
    let file = age.finish().map_err(|err| io_error(&err))?;
    Ok(armor(&file))
}

/// The plaintext of an armored age file: [`Error::Passphrase`] when `passphrase` doesn't open
/// it, [`Error::NotAge`] when it isn't one (or was cut or altered).
pub fn open(armored: &str, passphrase: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let file = dearmor(armored)?;
    let mut age = Decryptor::new(file.as_slice(), passphrase)?;
    let mut plain = Zeroizing::new(Vec::new());
    age.read_to_end(&mut plain)
        .map_err(|err| Error::NotAge(err.to_string()))?;
    Ok(plain)
}

/// `file` armored, with a final newline.
pub fn armor(file: &[u8]) -> String {
    let body = BASE64.encode(file);
    let mut out = String::with_capacity(body.len() + body.len() / COLUMNS + BEGIN.len() + 40);
    out.push_str(BEGIN);
    out.push('\n');
    for line in body.as_bytes().chunks(COLUMNS) {
        // Base64 is ASCII, so every chunk is a whole string.
        out.push_str(std::str::from_utf8(line).unwrap_or_default());
        out.push('\n');
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// The file an armor holds.
pub fn dearmor(armored: &str) -> Result<Vec<u8>> {
    if armored.len() > MAX_ARMORED {
        return Err(bad("the armor is too long"));
    }
    // `lines` takes LF and CRLF alike.
    let mut lines = armored.trim().lines();
    if lines.next() != Some(BEGIN) {
        return Err(bad("no BEGIN AGE ENCRYPTED FILE line"));
    }
    let lines: Vec<&str> = lines.collect();
    let Some((&last, body)) = lines.split_last() else {
        return Err(bad("no END AGE ENCRYPTED FILE line"));
    };
    if last != END {
        return Err(bad("no END AGE ENCRYPTED FILE line"));
    }
    let Some((&tail, full)) = body.split_last() else {
        return Err(bad("nothing between its BEGIN and END lines"));
    };
    if full.iter().any(|line| line.len() != COLUMNS) || tail.is_empty() || tail.len() > COLUMNS {
        return Err(bad("its lines aren't 64 columns"));
    }
    let mut text = String::with_capacity(COLUMNS * body.len());
    for line in body {
        text.push_str(line);
    }
    BASE64
        .decode(text.as_bytes())
        .map_err(|_| bad("its Base64 doesn't decode"))
}

fn bad(why: &str) -> Error {
    Error::NotAge(format!("armor: {why}"))
}

fn io_error(err: &std::io::Error) -> Error {
    Error::NotAge(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cheap work factor: the format is the same, only scrypt's cost differs.
    const LOG_N: u8 = 4;

    #[test]
    fn armored_files_round_trip() {
        for len in [0_u32, 1, 47, 48, 49, 1_000, 70_000] {
            let plain: Vec<u8> = (0..len).map(|i| u8::try_from(i % 251).unwrap()).collect();
            let armored = seal_with(&plain, b"correct horse", LOG_N).unwrap();
            assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----\n"));
            assert!(armored.ends_with("-----END AGE ENCRYPTED FILE-----\n"));
            assert!(armored.lines().all(|line| line.len() <= 64));
            assert_eq!(*open(&armored, b"correct horse").unwrap(), plain);
            let crlf = armored.replace('\n', "\r\n");
            assert_eq!(
                *open(&format!("  \n{crlf}\n"), b"correct horse").unwrap(),
                plain
            );
        }
    }

    #[test]
    fn age_s_file_opens_armored() {
        // age -a writes the PEM of the very bytes age writes without it.
        let armored = armor(include_bytes!("testdata/hello.age"));
        assert_eq!(*open(&armored, b"uniwrt").unwrap(), b"hello from age\n");
    }

    #[test]
    fn a_wrong_passphrase_and_bad_armor_are_told_apart() {
        let armored = seal_with(b"secret", b"correct horse", LOG_N).unwrap();
        assert!(matches!(open(&armored, b"wrong"), Err(Error::Passphrase)));
        let rewrapped = {
            let body: String = armored
                .lines()
                .skip(1)
                .take_while(|l| !l.starts_with("-----"))
                .collect();
            format!("{BEGIN}\n{body}\n{END}\n")
        };
        for broken in [
            armored.replacen("BEGIN", "START", 1),
            armored.replacen("-----END AGE ENCRYPTED FILE-----", "", 1),
            rewrapped,
            {
                let mut lines: Vec<String> = armored.lines().map(str::to_owned).collect();
                lines[1].replace_range(0..1, "*");
                lines.join("\n")
            },
            format!("{BEGIN}\n{END}\n"),
        ] {
            assert!(
                matches!(open(&broken, b"correct horse"), Err(Error::NotAge(_))),
                "{broken}"
            );
        }
    }
}
