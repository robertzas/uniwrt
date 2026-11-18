//! The age file format (`age-encryption.org/v1`) with one recipient, a passphrase (age's scrypt
//! stanza), so that a controller backup also opens without uniwrt: `age -d FILE | tar xz`
//! (features/lifecycle.md, Backups; D-129).
//!
//! - **Header:** the version line, one `-> scrypt <salt> <log2 N>` stanza whose body is the
//!   16-byte file key sealed with ChaCha20-Poly1305 (zero nonce) under scrypt(passphrase,
//!   `age-encryption.org/v1/scrypt` ‖ salt, N, r = 8, p = 1), then `---` and an HMAC-SHA256 of
//!   the header under HKDF(file key, "header").
//! - **Payload:** a 16-byte nonce, then STREAM: 64 KiB chunks sealed with ChaCha20-Poly1305
//!   under HKDF(file key, nonce, "payload"), each nonce an 11-byte counter and a last-chunk flag,
//!   so a truncated or reordered file fails.
//! - [`Encryptor`] writes it through any [`Write`](std::io::Write), [`Decryptor`] reads it from
//!   any [`Read`](std::io::Read), both in constant memory. Files of other recipients (X25519,
//!   plugins) are refused, as are work factors over [`MAX_LOG_N`].

mod stream;

use std::io::{self, BufRead, Write as _};

use data_encoding::BASE64_NOPAD;
use ring::{
    aead::{self, Aad, CHACHA20_POLY1305, LessSafeKey, UnboundKey},
    hkdf, hmac,
    rand::{SecureRandom as _, SystemRandom},
};
use zeroize::Zeroizing;

#[cfg(test)]
use stream::CHUNK as CHUNK_FOR_TESTS;
pub use stream::{Decryptor, Encryptor};

use crate::{Error, Result};

/// The scrypt work factor written: age's own default (N = 2¹⁸, 256 MiB for about a second).
pub const LOG_N: u8 = 18;
/// The highest work factor read (N = 2²⁰, 1 GiB): what a small controller can still afford.
pub const MAX_LOG_N: u8 = 20;

const VERSION: &[u8] = b"age-encryption.org/v1";
const SCRYPT_LABEL: &[u8] = b"age-encryption.org/v1/scrypt";
const FILE_KEY: usize = 16;
const SALT: usize = 16;
const NONCE: usize = 16;
const TAG: usize = 16;
/// A stanza's body is wrapped at 64 columns; a shorter line ends it.
const COLUMNS: usize = 64;
/// The longest header line read: far more than a scrypt stanza needs.
const MAX_LINE: usize = 256;

type FileKey = Zeroizing<[u8; FILE_KEY]>;

fn random(bytes: &mut [u8]) -> Result<()> {
    SystemRandom::new().fill(bytes).map_err(|_| Error::Random)
}

/// HKDF-SHA256 of the file key: 32 bytes for `info`.
fn derive(file_key: &FileKey, salt: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]> {
    struct Len32;
    impl hkdf::KeyType for Len32 {
        fn len(&self) -> usize {
            32
        }
    }
    let mut out = Zeroizing::new([0; 32]);
    // Cannot fail: 32 bytes is far below HKDF-SHA256's limit of 255 × 32.
    hkdf::Salt::new(hkdf::HKDF_SHA256, salt)
        .extract(file_key.as_ref())
        .expand(&[info], Len32)
        .and_then(|okm| okm.fill(out.as_mut()))
        .expect("HKDF output of 32 bytes");
    out
}

fn chacha(key: &[u8; 32]) -> LessSafeKey {
    // Cannot fail: the key has ChaCha20-Poly1305's length.
    LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, key).expect("a 32-byte key"))
}

/// The key that wraps the file key: scrypt of the passphrase.
fn wrapping_key(passphrase: &[u8], salt: &[u8; SALT], log_n: u8) -> Result<LessSafeKey> {
    let mut label = Vec::with_capacity(SCRYPT_LABEL.len() + SALT);
    label.extend_from_slice(SCRYPT_LABEL);
    label.extend_from_slice(salt);
    let params =
        scrypt::Params::new(log_n, 8, 1).map_err(|_| malformed("bad scrypt work factor"))?;
    let mut key = Zeroizing::new([0; 32]);
    scrypt::scrypt(passphrase, &label, &params, key.as_mut())
        .map_err(|_| malformed("bad scrypt output length"))?;
    Ok(chacha(&key))
}

fn malformed(why: &str) -> Error {
    Error::NotAge(why.to_owned())
}

/// The header of a new file and the file key it wraps.
fn seal_header(passphrase: &[u8], log_n: u8) -> Result<(Vec<u8>, FileKey)> {
    let mut file_key = FileKey::default();
    random(file_key.as_mut())?;
    let mut salt = [0; SALT];
    random(&mut salt)?;
    let mut body = file_key.to_vec();
    wrapping_key(passphrase, &salt, log_n)?
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key([0; 12]),
            Aad::empty(),
            &mut body,
        )
        .map_err(|_| malformed("cannot wrap the file key"))?;
    let mut header = Vec::with_capacity(160);
    header.extend_from_slice(VERSION);
    // Writes to a Vec cannot fail.
    let _ = write!(
        header,
        "\n-> scrypt {} {log_n}\n{}\n---",
        BASE64_NOPAD.encode(&salt),
        BASE64_NOPAD.encode(&body)
    );
    let mac = hmac::sign(&header_key(&file_key), &header);
    header.push(b' ');
    header.extend_from_slice(BASE64_NOPAD.encode(mac.as_ref()).as_bytes());
    header.push(b'\n');
    Ok((header, file_key))
}

fn header_key(file_key: &FileKey) -> hmac::Key {
    hmac::Key::new(hmac::HMAC_SHA256, derive(file_key, &[], b"header").as_ref())
}

/// Reads and checks the header, then unwraps the file key with `passphrase`.
fn open_header(input: &mut impl BufRead, passphrase: &[u8]) -> Result<FileKey> {
    let mut header = Vec::with_capacity(160);
    if line(input, &mut header)? != VERSION {
        return Err(malformed("not an age file"));
    }
    let stanza = line(input, &mut header)?;
    let args: Vec<&[u8]> = stanza
        .strip_prefix(b"-> ")
        .ok_or_else(|| malformed("no recipient"))?
        .split(|&b| b == b' ')
        .collect();
    let [b"scrypt", salt, log_n] = args.as_slice() else {
        return Err(malformed("not encrypted with a passphrase"));
    };
    let salt: [u8; SALT] = decode(salt)?
        .try_into()
        .map_err(|_| malformed("bad scrypt salt"))?;
    let log_n = std::str::from_utf8(log_n)
        .ok()
        .filter(|n| !n.starts_with('0'))
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=MAX_LOG_N).contains(n))
        .ok_or_else(|| malformed("bad or too costly scrypt work factor"))?;
    let mut body = decode(&line(input, &mut header)?)?;
    if body.len() != FILE_KEY + TAG {
        return Err(malformed("bad scrypt stanza body"));
    }
    // The MAC covers the header up to and including `---`, without the space after it.
    let covered = header.len() + 3;
    let mac = line(input, &mut header)?;
    let Some(mac) = mac.strip_prefix(b"--- ") else {
        return Err(malformed("more than one recipient"));
    };
    let mac = decode(mac)?;
    header.truncate(covered);
    let file_key = wrapping_key(passphrase, &salt, log_n)?
        .open_in_place(
            aead::Nonce::assume_unique_for_key([0; 12]),
            Aad::empty(),
            &mut body,
        )
        .map_err(|_| Error::Passphrase)
        .map(|key| {
            let mut file_key = FileKey::default();
            file_key.copy_from_slice(key);
            file_key
        })?;
    body.fill(0);
    hmac::verify(&header_key(&file_key), &header, &mac)
        .map_err(|_| malformed("the header was altered"))?;
    Ok(file_key)
}

/// One header line, without its newline; the raw line, newline included, is added to `header`.
fn line(input: &mut impl BufRead, header: &mut Vec<u8>) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    let read = io::Read::take(&mut *input, MAX_LINE as u64)
        .read_until(b'\n', &mut line)
        .map_err(|source| Error::Io {
            what: "the encrypted file".to_owned(),
            source,
        })?;
    if read == 0 || line.pop() != Some(b'\n') {
        return Err(malformed("truncated or overlong header"));
    }
    header.extend_from_slice(&line);
    header.push(b'\n');
    Ok(line)
}

fn decode(text: &[u8]) -> Result<Vec<u8>> {
    if text.len() >= COLUMNS {
        return Err(malformed("overlong header field"));
    }
    BASE64_NOPAD
        .decode(text)
        .map_err(|_| malformed("bad base64 in the header"))
}

fn io_error(error: Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests;
