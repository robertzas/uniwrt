//! Root's password on a node (features/adoption.md, Identity, credentials, trust): adoption gives
//! a node that has none (factory-default OpenWrt, whose dropbear lets anyone on its LAN in as
//! root) a random one, which the controller keeps in its vault for emergencies.

use ring::rand::{SecureRandom as _, SystemRandom};
use zeroize::Zeroizing;

use crate::{Error, ExecRequest, Result};

/// Lower-case letters and digits that can't be mistaken for one another when read off a
/// screen (no `i`, `l`, `o`, `0`, `1`).
const ALPHABET: &[u8; 31] = b"abcdefghjkmnpqrstuvwxyz23456789";
const GROUPS: usize = 6;
const GROUP: usize = 4;

/// A new random password: six groups of four characters joined by dashes (118 bits), easy to
/// type into LuCI or an SSH prompt.
pub fn generate() -> Result<Zeroizing<String>> {
    let random = SystemRandom::new();
    let mut password = Zeroizing::new(String::with_capacity(GROUPS * (GROUP + 1)));
    let mut byte = Zeroizing::new([0_u8; 1]);
    let mut drawn = 0;
    while drawn < GROUPS * GROUP {
        random.fill(byte.as_mut()).map_err(|_| Error::Random)?;
        // 248 is the largest multiple of 31 a byte holds: dropping the bytes above it keeps
        // every character equally likely.
        if byte[0] >= 248 {
            continue;
        }
        if drawn > 0 && drawn % GROUP == 0 {
            password.push('-');
        }
        password.push(char::from(ALPHABET[usize::from(byte[0] % 31)]));
        drawn += 1;
    }
    Ok(password)
}

/// Sets root's password to `password` with BusyBox's `passwd`, which reads it twice from stdin
/// (never from argv, which other users can see in `ps`).
pub fn set(password: &str) -> ExecRequest {
    let mut stdin = Zeroizing::new(String::with_capacity(2 * (password.len() + 1)));
    for _ in 0..2 {
        stdin.push_str(password);
        stdin.push('\n');
    }
    ExecRequest::new(["passwd", "root"]).stdin(stdin.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_six_groups_of_four_and_random() {
        let one = generate().unwrap();
        let groups: Vec<_> = one.split('-').collect();
        assert_eq!(groups.len(), GROUPS, "{}", one.as_str());
        for group in groups {
            assert_eq!(group.len(), GROUP);
            assert!(group.bytes().all(|c| ALPHABET.contains(&c)));
        }
        assert_ne!(one, generate().unwrap());
    }

    #[test]
    fn the_password_goes_to_passwd_on_stdin_twice() {
        let request = set("abcd-efgh");
        assert_eq!(request.argv, ["passwd", "root"]);
        assert_eq!(
            request.stdin.as_deref(),
            Some(b"abcd-efgh\nabcd-efgh\n".as_slice())
        );
    }
}
