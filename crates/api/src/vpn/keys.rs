//! WireGuard keys (features/network.md, VPN; D-140), made by the controller as a save or an
//! import gives the site its next revision ([`fill`]):
//!
//! - the server gets a keypair when it has no private key, and a peer one when it has neither a
//!   private nor a public key (a peer that brings its own keypair gives only its public key);
//!   every peer without a preshared key gets one. New keys are sealed under new names and
//!   committed with the revision.
//! - **Public keys follow private keys:** a private key the latest revision holds keeps that
//!   revision's public key; any other one (set by the save, or a reference copied from an older
//!   revision) has its public key derived again, so a document can't pair a private key with
//!   another public key. To bring its own keypair, a peer's private key is cleared.
//!
//! Private keys are 32 random bytes from ring, clamped as `wg genkey` does; public keys are
//! X25519 of the base point (curve25519-dalek); both in padded base64, as `wg` prints them.

use std::collections::BTreeMap;

use curve25519_dalek::montgomery::MontgomeryPoint;
use data_encoding::BASE64;
use ring::rand::{SecureRandom as _, SystemRandom};
use uniwrt_model::site::{
    SecretRef, SiteConfig,
    vpn::{Peer, WireGuard},
};
use uniwrt_store::{SealedSecret, Vault, VaultRepo as _};
use zeroize::Zeroizing;

use crate::{Api, ApiError, ApiResult};

/// Gives `config`'s server and peers the keys they lack and the public keys their private keys
/// make, sealing the new keys. `head` is the latest revision's server; `known` holds the values
/// of references the vault doesn't hold yet (the save's or the import's own secrets).
pub(crate) async fn fill(
    api: &Api,
    config: &mut SiteConfig,
    head: Option<&WireGuard>,
    known: &BTreeMap<SecretRef, Zeroizing<String>>,
) -> ApiResult<Vec<SealedSecret>> {
    let Some(server) = config.vpn.wireguard.as_mut() else {
        return Ok(Vec::new());
    };
    let vault = api.vault()?;
    let mut keys = Keys {
        vault,
        known,
        sealed: Vec::new(),
    };
    let kept = head.and_then(|was| Some((was.private_key.as_ref()?, was.public_key.as_ref()?)));
    server.public_key = Some(match (&server.private_key, kept) {
        (None, _) => {
            let (private, public) = keypair()?;
            server.private_key = Some(keys.seal(&private)?);
            public
        }
        (Some(private), Some((was, public))) if private == was => public.clone(),
        (Some(private), _) => keys.public("vpn/wireguard/private_key", private).await?,
    });
    for peer in &mut server.peers {
        let before = head.and_then(|head| head.peers.iter().find(|p| p.id == peer.id));
        keys.peer(peer, before).await?;
    }
    Ok(keys.sealed)
}

struct Keys<'a> {
    vault: &'a Vault,
    known: &'a BTreeMap<SecretRef, Zeroizing<String>>,
    sealed: Vec<SealedSecret>,
}

impl Keys<'_> {
    async fn peer(&mut self, peer: &mut Peer, before: Option<&Peer>) -> ApiResult<()> {
        if peer.preshared_key.is_none() {
            peer.preshared_key = Some(self.seal(&random_key()?)?);
        }
        match &peer.private_key {
            None if peer.public_key.is_empty() => {
                let (private, public) = keypair()?;
                peer.private_key = Some(self.seal(&private)?);
                peer.public_key = public;
            }
            // Its own keypair.
            None => {}
            Some(private) => match before {
                Some(before) if before.private_key.as_ref() == Some(private) => {
                    peer.public_key.clone_from(&before.public_key);
                }
                _ => {
                    let path = format!("vpn/wireguard/peers/{}/private_key", peer.id);
                    peer.public_key = self.public(&path, private).await?;
                }
            },
        }
        Ok(())
    }

    fn seal(&mut self, value: &str) -> ApiResult<SecretRef> {
        let secret = self.vault.seal_config_secret(value.as_bytes())?;
        let reference = secret.reference().clone();
        self.sealed.push(secret);
        Ok(reference)
    }

    /// The public key of the private key at `path`, read from the save or the vault.
    async fn public(&self, path: &str, private: &SecretRef) -> ApiResult<String> {
        let value = match self.known.get(private) {
            Some(value) => Zeroizing::new(value.as_bytes().to_vec()),
            None => self.vault.secret(private.name()).await?.ok_or_else(|| {
                ApiError::BadRequest(format!("The vault holds no secret for {path}"))
            })?,
        };
        std::str::from_utf8(&value)
            .ok()
            .and_then(public_key)
            .ok_or_else(|| ApiError::BadRequest(format!("{path} isn't a WireGuard key")))
    }
}

/// The public key of `private`, a key in base64, if it is one.
pub(crate) fn public_key(private: &str) -> Option<String> {
    let bytes = Zeroizing::new(BASE64.decode(private.as_bytes()).ok()?);
    let scalar: Zeroizing<[u8; 32]> = Zeroizing::new(bytes.as_slice().try_into().ok()?);
    Some(BASE64.encode(MontgomeryPoint::mul_base_clamped(*scalar).as_bytes()))
}

/// A new private key, clamped, and its public key.
fn keypair() -> ApiResult<(Zeroizing<String>, String)> {
    let mut bytes = random_bytes()?;
    bytes[0] &= 0xf8;
    bytes[31] = (bytes[31] & 0x7f) | 0x40;
    let private = Zeroizing::new(BASE64.encode(bytes.as_slice()));
    let public = BASE64.encode(MontgomeryPoint::mul_base_clamped(*bytes).as_bytes());
    Ok((private, public))
}

/// A preshared key: 32 random bytes.
fn random_key() -> ApiResult<Zeroizing<String>> {
    Ok(Zeroizing::new(BASE64.encode(random_bytes()?.as_slice())))
}

fn random_bytes() -> ApiResult<Zeroizing<[u8; 32]>> {
    let mut bytes = Zeroizing::new([0_u8; 32]);
    SystemRandom::new()
        .fill(bytes.as_mut_slice())
        .map_err(|_| ApiError::Internal("the random number generator failed".to_owned()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use data_encoding::{BASE64, HEXLOWER};
    use uniwrt_model::site::vpn::is_key;

    use super::{keypair, public_key, random_key};

    #[test]
    fn public_keys_are_x25519_of_the_base_point() {
        // RFC 7748, section 6.1: Alice's keys.
        let hex = |s: &str| BASE64.encode(&HEXLOWER.decode(s.as_bytes()).unwrap());
        let private = hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let public = hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a");
        assert_eq!(public_key(&private), Some(public));
        assert_eq!(public_key("not a key"), None);
        assert_eq!(public_key(&BASE64.encode(&[1; 31])), None);
    }

    #[test]
    fn new_keys_are_clamped_wireguard_keys() {
        let (private, public) = keypair().unwrap();
        assert!(is_key(&private) && is_key(&public));
        let bytes = BASE64.decode(private.as_bytes()).unwrap();
        assert_eq!((bytes[0] & 7, bytes[31] & 0xc0), (0, 0x40));
        assert_eq!(public_key(&private), Some(public));
        let (psk, other) = (random_key().unwrap(), random_key().unwrap());
        assert!(is_key(&psk) && psk != other);
    }
}
