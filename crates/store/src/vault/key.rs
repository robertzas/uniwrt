//! The master key: 32 random bytes that every secret in the vault is sealed under, kept outside
//! the database (`master.key` or `UNIWRT_MASTER_KEY`; features/security.md, Secrets vault).

use std::fmt;

use ring::rand::{SecureRandom as _, SystemRandom};
use zeroize::Zeroizing;

use crate::{Error, Result};

const LEN: usize = 32;

/// The vault's master key. Its text form is 64 hexadecimal digits.
pub struct MasterKey(Zeroizing<[u8; LEN]>);

impl MasterKey {
    /// A new key from the system's random number generator.
    pub fn generate() -> Result<Self> {
        let mut key = Zeroizing::new([0; LEN]);
        SystemRandom::new()
            .fill(key.as_mut())
            .map_err(|_| Error::Random)?;
        Ok(Self(key))
    }

    /// Parses the text form; surrounding whitespace (a file's final newline) is ignored.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim().as_bytes();
        if text.len() != 2 * LEN {
            return Err(Error::MasterKey);
        }
        let mut key = Zeroizing::new([0; LEN]);
        for (byte, [high, low]) in key.iter_mut().zip(text.as_chunks::<2>().0) {
            *byte = (hex_digit(*high)? << 4) | hex_digit(*low)?;
        }
        Ok(Self(key))
    }

    /// The text form: 64 lowercase hexadecimal digits.
    pub fn to_text(&self) -> Zeroizing<String> {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = Zeroizing::new(String::with_capacity(2 * LEN));
        for byte in self.0.iter() {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 0xf)]));
        }
        text
    }

    pub(crate) fn bytes(&self) -> &[u8; LEN] {
        &self.0
    }
}

fn hex_digit(digit: u8) -> Result<u8> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err(Error::MasterKey),
    }
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterKey(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::MasterKey;
    use crate::Error;

    #[test]
    fn the_text_form_round_trips() {
        let key = MasterKey::generate().unwrap();
        let text = key.to_text();
        assert_eq!(text.len(), 64);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        let again = MasterKey::parse(&format!("  {}\n", text.to_uppercase())).unwrap();
        assert_eq!(again.bytes(), key.bytes());
        assert_ne!(MasterKey::generate().unwrap().bytes(), key.bytes());
    }

    #[test]
    fn malformed_keys_are_refused() {
        for text in ["", "00", &"0".repeat(63), &"0".repeat(65), &"g".repeat(64)] {
            assert!(
                matches!(MasterKey::parse(text), Err(Error::MasterKey)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn debug_does_not_show_the_key() {
        assert_eq!(
            format!("{:?}", MasterKey::generate().unwrap()),
            "MasterKey(..)"
        );
    }
}
