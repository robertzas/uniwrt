use std::io::{Read as _, Write as _};

use super::{CHUNK_FOR_TESTS as CHUNK, Decryptor, Encryptor};
use crate::Error;

/// A cheap work factor: the format is the same, only scrypt's cost differs.
const LOG_N: u8 = 4;

fn encrypt(plain: &[u8], passphrase: &str) -> Vec<u8> {
    let mut age = Encryptor::new(Vec::new(), passphrase.as_bytes(), LOG_N).unwrap();
    // Uneven writes cross the chunk boundaries anywhere.
    for part in plain.chunks(7_777) {
        age.write_all(part).unwrap();
    }
    age.finish().unwrap()
}

fn decrypt(file: &[u8], passphrase: &str) -> std::io::Result<Vec<u8>> {
    let mut age = Decryptor::new(file, passphrase.as_bytes()).map_err(std::io::Error::other)?;
    let mut plain = Vec::new();
    age.read_to_end(&mut plain)?;
    Ok(plain)
}

fn data(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from(i * 31 % 251).unwrap())
        .collect()
}

#[test]
fn files_of_any_length_round_trip() {
    for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK, 3 * CHUNK + 5] {
        let plain = data(len);
        let file = encrypt(&plain, "correct horse");
        let chunks = len.div_ceil(CHUNK).max(1);
        assert!(file.len() > len + 16 * chunks, "{len}");
        assert_eq!(decrypt(&file, "correct horse").unwrap(), plain, "{len}");
    }
}

#[test]
fn the_header_is_age_s_scrypt_stanza() {
    let file = encrypt(b"x", "pass");
    let text = String::from_utf8_lossy(&file);
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("age-encryption.org/v1"));
    let stanza: Vec<_> = lines.next().unwrap().split(' ').collect();
    assert_eq!(stanza[..2], ["->", "scrypt"]);
    assert_eq!((stanza[2].len(), stanza[3]), (22, "4"));
    assert_eq!(lines.next().unwrap().len(), 43);
    assert!(lines.next().unwrap().starts_with("--- "));
    // Two files of the same plaintext share nothing.
    assert_ne!(encrypt(b"x", "pass")[..60], file[..60]);
}

#[test]
fn a_wrong_passphrase_is_told_apart_from_a_bad_file() {
    let file = encrypt(b"secret", "right");
    assert!(matches!(
        Decryptor::new(file.as_slice(), b"wrong"),
        Err(Error::Passphrase)
    ));
    for bad in [
        &b"hello"[..],
        b"",
        b"age-encryption.org/v1\n-> X25519 abc\nxyz\n--- abc\n",
    ] {
        assert!(
            matches!(Decryptor::new(bad, b"x"), Err(Error::NotAge(_))),
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
    // A costlier work factor than a small controller affords.
    let text = String::from_utf8(file[..file.len() - 16 - 6 - 16].to_vec()).unwrap();
    let costly = text.replacen(" 4\n", " 21\n", 1);
    assert!(matches!(
        Decryptor::new(costly.as_bytes(), b"right"),
        Err(Error::NotAge(why)) if why.contains("work factor")
    ));
}

#[test]
fn an_altered_header_fails_its_mac() {
    let mut file = encrypt(b"secret", "pass");
    let at = file.windows(4).position(|w| w == b"--- ").unwrap() + 4;
    file[at] = if file[at] == b'A' { b'B' } else { b'A' };
    assert!(matches!(
        Decryptor::new(file.as_slice(), b"pass"),
        Err(Error::NotAge(_))
    ));
}

#[test]
fn a_truncated_or_altered_payload_fails() {
    let plain = data(2 * CHUNK);
    let file = encrypt(&plain, "pass");
    // Cut at the end of the first chunk: it was sealed as not the last.
    let header = file.len() - (2 * CHUNK + 32);
    assert!(decrypt(&file[..header + CHUNK + 16], "pass").is_err());
    assert!(decrypt(&file[..file.len() - 1], "pass").is_err());
    let mut altered = file.clone();
    altered[header + 100] ^= 1;
    assert!(decrypt(&altered, "pass").is_err());
    let mut longer = file;
    longer.push(0);
    assert!(decrypt(&longer, "pass").is_err());
}

/// Written by age 1.3.2 (`age -p`, passphrase `uniwrt`, plaintext `hello from age\n`): this reads
/// what age writes. That age reads what this writes was checked with `age -d` (D-129).
#[test]
fn a_file_written_by_age_opens() {
    let file = include_bytes!("testdata/hello.age");
    assert_eq!(decrypt(file, "uniwrt").unwrap(), b"hello from age\n");
}
