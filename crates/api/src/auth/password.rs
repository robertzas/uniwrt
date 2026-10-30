//! Passwords: the policy for new ones, and Argon2id hashing and verification on the blocking
//! pool, at most [`CONCURRENT`] at once so that a burst of logins cannot exhaust a small
//! controller's memory (each takes [`PasswordCost::memory_kib`]).

use std::sync::Arc;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher as _, PasswordVerifier as _},
};
use ring::rand::{SecureRandom as _, SystemRandom};
use tokio::sync::{OnceCell, Semaphore};
use zeroize::Zeroizing;

use crate::ApiError;

/// Hashes computed at once, at most.
pub(crate) const CONCURRENT: usize = 2;
/// NIST SP 800-63B asks for at least 8 characters, and 15 for passwords used alone; the
/// controller is a LAN service behind rate limits, so 12, with no composition rules.
pub(crate) const MIN_PASSWORD_CHARS: usize = 12;
/// Long enough for any passphrase, short enough to bound the hashing work.
pub(crate) const MAX_PASSWORD_BYTES: usize = 1_024;

/// Argon2id's cost. The default is OWASP's minimum (19 MiB, 2 passes, 1 lane): about 50 ms on
/// a Raspberry Pi 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasswordCost {
    pub memory_kib: u32,
    pub iterations: u32,
}

impl Default for PasswordCost {
    fn default() -> Self {
        Self {
            memory_kib: 19 * 1_024,
            iterations: 2,
        }
    }
}

impl PasswordCost {
    /// As cheap as Argon2 allows, for tests.
    pub const CHEAPEST: Self = Self {
        memory_kib: 8,
        iterations: 1,
    };
}

/// Hashes and verifies passwords.
pub(crate) struct Passwords {
    argon2: Arc<Argon2<'static>>,
    permits: Semaphore,
    /// A hash verified when the user does not exist, so that answers take as long either way.
    dummy: OnceCell<String>,
}

impl Passwords {
    pub(crate) fn new(cost: PasswordCost) -> Result<Self, ApiError> {
        let params = Params::new(cost.memory_kib, cost.iterations, 1, None)
            .map_err(|err| ApiError::Internal(format!("bad Argon2 parameters: {err}")))?;
        Ok(Self {
            argon2: Arc::new(Argon2::new(Algorithm::Argon2id, Version::V0x13, params)),
            permits: Semaphore::new(CONCURRENT),
            dummy: OnceCell::new(),
        })
    }

    /// Computes the dummy hash now, so that the first login of an unknown user takes no longer
    /// than any other.
    pub(crate) async fn warm_up(&self) -> Result<(), ApiError> {
        self.dummy().await.map(|_| ())
    }

    async fn dummy(&self) -> Result<&String, ApiError> {
        self.dummy
            .get_or_try_init(|| self.hash(Zeroizing::new("uniwrt dummy password".to_owned())))
            .await
    }

    /// The PHC string (`$argon2id$v=19$m=…`) of `password`, with a fresh 128-bit salt.
    pub(crate) async fn hash(&self, password: Zeroizing<String>) -> Result<String, ApiError> {
        let mut salt = [0; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| ApiError::Internal("the random number generator failed".to_owned()))?;
        let argon2 = Arc::clone(&self.argon2);
        self.blocking(move || {
            argon2
                .hash_password_with_salt(password.as_bytes(), &salt)
                .map(|hash| hash.to_string())
                .map_err(|err| ApiError::Internal(format!("cannot hash a password: {err}")))
        })
        .await
    }

    /// Whether `password` matches `hash`; with no hash (an unknown user), a dummy is checked
    /// instead and the answer is no.
    pub(crate) async fn verify(
        &self,
        password: Zeroizing<String>,
        hash: Option<String>,
    ) -> Result<bool, ApiError> {
        let (hash, known) = if let Some(hash) = hash {
            (hash, true)
        } else {
            (self.dummy().await?.clone(), false)
        };
        // The hash's own parameters apply, so hashes made at an older cost still verify.
        let argon2 = Arc::clone(&self.argon2);
        let matches = self
            .blocking(move || {
                Ok(argon2
                    .verify_password(password.as_bytes(), hash.as_str())
                    .is_ok())
            })
            .await?;
        Ok(known && matches)
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| ApiError::Internal("the password hasher is closed".to_owned()))?;
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|err| ApiError::Internal(format!("password hashing failed: {err}")))?
    }
}

/// Checks a new password against the policy.
pub(crate) fn check_new(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(ApiError::BadRequest(format!(
            "The password must have at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(ApiError::BadRequest(format!(
            "The password must have at most {MAX_PASSWORD_BYTES} bytes"
        )));
    }
    Ok(())
}

/// A username as stored: lowercase letters, digits, `.`, `_` and `-`, 1 to 64 of them.
pub(crate) fn username(name: &str) -> Result<String, ApiError> {
    let name = name.trim().to_ascii_lowercase();
    let valid = (1..=64).contains(&name.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    if valid {
        Ok(name)
    } else {
        Err(ApiError::BadRequest(
            "A username has 1 to 64 letters, digits, dots, underscores or hyphens".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use zeroize::Zeroizing;

    use super::{PasswordCost, Passwords, check_new, username};

    fn secret(text: &str) -> Zeroizing<String> {
        Zeroizing::new(text.to_owned())
    }

    #[tokio::test]
    async fn hashes_verify_only_their_password() {
        let passwords = Passwords::new(PasswordCost::CHEAPEST).unwrap();
        let hash = passwords.hash(secret("correct horse")).await.unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=8,t=1,p=1$"), "{hash}");
        assert!(
            passwords
                .verify(secret("correct horse"), Some(hash.clone()))
                .await
                .unwrap()
        );
        assert!(
            !passwords
                .verify(secret("wrong horse"), Some(hash.clone()))
                .await
                .unwrap()
        );
        assert!(
            !passwords
                .verify(secret("correct horse"), None)
                .await
                .unwrap()
        );
        assert!(
            !passwords
                .verify(secret("x"), Some("not a hash".into()))
                .await
                .unwrap()
        );
        assert_ne!(
            passwords.hash(secret("correct horse")).await.unwrap(),
            hash,
            "salted"
        );

        // A hash made at another cost verifies by its own parameters.
        let stronger = Passwords::new(PasswordCost {
            memory_kib: 64,
            iterations: 2,
        })
        .unwrap();
        let hash = stronger.hash(secret("correct horse")).await.unwrap();
        assert!(
            passwords
                .verify(secret("correct horse"), Some(hash))
                .await
                .unwrap()
        );
    }

    #[test]
    fn the_production_cost_is_owasps_minimum() {
        assert_eq!(
            PasswordCost::default(),
            PasswordCost {
                memory_kib: 19_456,
                iterations: 2
            }
        );
        Passwords::new(PasswordCost::default()).unwrap();
    }

    #[test]
    fn new_passwords_and_usernames_follow_the_policy() {
        assert!(check_new("twelve chars").is_ok());
        assert!(check_new("eleven char").is_err());
        assert!(check_new("ünïcödé-pässwörd").is_ok());
        assert!(check_new(&"x".repeat(1_025)).is_err());
        assert_eq!(username(" Admin ").unwrap(), "admin");
        assert_eq!(username("robert.z_1-a").unwrap(), "robert.z_1-a");
        for bad in ["", "a b", "root@lan", &"a".repeat(65)] {
            assert!(username(bad).is_err(), "{bad:?}");
        }
    }
}
