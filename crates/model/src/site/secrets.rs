//! The document's secret slots: where a [`SecretRef`] may sit, by path, and what each value
//! may be.

use super::{SecretRef, SiteConfig, gateway::WanProto, node, vpn::is_key};

/// One place in the document that holds a secret.
#[derive(Debug, PartialEq, Eq)]
pub struct SecretSlot<'a> {
    /// Its path, such as `wlans/home/passphrase`, `mesh/key` or `wan/password`.
    pub path: String,
    pub value: Option<&'a SecretRef>,
}

impl SiteConfig {
    /// Every slot that can hold a secret, set or not, in document order.
    pub fn secrets(&self) -> Vec<SecretSlot<'_>> {
        let mut slots: Vec<SecretSlot<'_>> = self
            .wlans
            .iter()
            .map(|wlan| SecretSlot {
                path: format!("wlans/{}/passphrase", wlan.id),
                value: wlan.passphrase.as_ref(),
            })
            .collect();
        slots.push(SecretSlot {
            path: "mesh/key".to_owned(),
            value: self.mesh.key.as_ref(),
        });
        if let WanProto::Pppoe { password, .. } = &self.wan.proto {
            slots.push(SecretSlot {
                path: "wan/password".to_owned(),
                value: password.as_ref(),
            });
        }
        slots.extend(self.services.ddns.iter().map(|ddns| SecretSlot {
            path: format!("services/ddns/{}/secret", ddns.id),
            value: ddns.secret.as_ref(),
        }));
        if let Some(wireguard) = &self.vpn.wireguard {
            slots.push(SecretSlot {
                path: "vpn/wireguard/private_key".to_owned(),
                value: wireguard.private_key.as_ref(),
            });
            for peer in &wireguard.peers {
                for (key, value) in [
                    ("private_key", &peer.private_key),
                    ("preshared_key", &peer.preshared_key),
                ] {
                    slots.push(SecretSlot {
                        path: format!("vpn/wireguard/peers/{}/{key}", peer.id),
                        value: value.as_ref(),
                    });
                }
            }
        }
        for config in &self.nodes {
            for (i, raw) in config.raw.iter().enumerate() {
                for (option, value) in &raw.options {
                    if let node::RawValue::Secret { secret } = value {
                        slots.push(SecretSlot {
                            path: format!("nodes/{}/raw/{i}/{option}", config.node),
                            value: Some(secret),
                        });
                    }
                }
            }
        }
        slots
    }

    /// Points every slot that holds `from` at `to` instead: a secret sealed under a new name.
    pub fn replace_secret(&mut self, from: &SecretRef, to: &SecretRef) {
        let wlans = self.wlans.iter_mut().filter_map(|w| w.passphrase.as_mut());
        let wan = match &mut self.wan.proto {
            WanProto::Pppoe { password, .. } => password.as_mut(),
            _ => None,
        };
        let raw = self
            .nodes
            .iter_mut()
            .flat_map(|config| config.raw.iter_mut())
            .flat_map(|raw| raw.options.values_mut())
            .filter_map(|value| match value {
                node::RawValue::Secret { secret } => Some(secret),
                _ => None,
            });
        let ddns = self
            .services
            .ddns
            .iter_mut()
            .filter_map(|d| d.secret.as_mut());
        let vpn = self.vpn.wireguard.iter_mut().flat_map(|wireguard| {
            let peers = wireguard
                .peers
                .iter_mut()
                .flat_map(|peer| [&mut peer.private_key, &mut peer.preshared_key]);
            std::iter::once(&mut wireguard.private_key)
                .chain(peers)
                .filter_map(Option::as_mut)
        });
        for secret in wlans
            .chain(self.mesh.key.as_mut())
            .chain(wan)
            .chain(ddns)
            .chain(vpn)
            .chain(raw)
        {
            if secret == from {
                *secret = to.clone();
            }
        }
    }

    /// The slot at `path` ([`SiteConfig::secrets`]), to set or clear; not a raw section's, which
    /// only brownfield import fills ([`SiteConfig::replace_secret`]).
    pub fn secret_mut(&mut self, path: &str) -> Option<&mut Option<SecretRef>> {
        match path.split('/').collect::<Vec<_>>().as_slice() {
            ["wlans", id, "passphrase"] => self
                .wlans
                .iter_mut()
                .find(|wlan| wlan.id == *id)
                .map(|wlan| &mut wlan.passphrase),
            ["mesh", "key"] => Some(&mut self.mesh.key),
            ["wan", "password"] => match &mut self.wan.proto {
                WanProto::Pppoe { password, .. } => Some(password),
                _ => None,
            },
            ["services", "ddns", id, "secret"] => self
                .services
                .ddns
                .iter_mut()
                .find(|ddns| ddns.id == *id)
                .map(|ddns| &mut ddns.secret),
            ["vpn", "wireguard", "private_key"] => {
                self.vpn.wireguard.as_mut().map(|w| &mut w.private_key)
            }
            ["vpn", "wireguard", "peers", id, key] => {
                let peer = self
                    .vpn
                    .wireguard
                    .as_mut()?
                    .peers
                    .iter_mut()
                    .find(|peer| peer.id == *id)?;
                match *key {
                    "private_key" => Some(&mut peer.private_key),
                    "preshared_key" => Some(&mut peer.preshared_key),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The secret at `path`, if set.
    pub fn secret(&self, path: &str) -> Option<&SecretRef> {
        self.secrets()
            .into_iter()
            .find(|slot| slot.path == path)
            .and_then(|slot| slot.value)
    }
}

/// Whether `value` suits the secret at `path`: a Wi-Fi passphrase or the mesh key is 8 to 63
/// printable ASCII characters (a WLAN's may instead be 64 hexadecimal digits, a raw PSK), a PPPoE
/// password 1 to 128 characters without control characters, a DDNS token or password 1 to 256
/// printable ASCII characters, a WireGuard key 32 bytes in base64 ([`is_key`]), and a raw
/// section's any value without line breaks.
pub fn check_secret(path: &str, value: &str) -> Result<(), &'static str> {
    let printable = |value: &str| value.bytes().all(|b| (b' '..=b'~').contains(&b));
    if path.starts_with("nodes/") {
        return if value.contains(['\0', '\n', '\r']) {
            Err("A value holds no line breaks or NUL characters")
        } else {
            Ok(())
        };
    }
    if path == "wan/password" {
        return if value.is_empty() || value.chars().count() > 128 {
            Err("A PPPoE password is 1 to 128 characters")
        } else if value.chars().any(char::is_control) {
            Err("A PPPoE password has no control characters")
        } else {
            Ok(())
        };
    }
    if path.starts_with("vpn/wireguard/") {
        return if is_key(value) {
            Ok(())
        } else {
            Err("A WireGuard key is 32 bytes in base64, 44 characters")
        };
    }
    if path.starts_with("services/ddns/") {
        return if value.is_empty() || value.len() > 256 || !printable(value) {
            Err("A DDNS token or password is 1 to 256 printable ASCII characters")
        } else {
            Ok(())
        };
    }
    let raw_psk = path.starts_with("wlans/")
        && value.len() == 64
        && value.bytes().all(|b| b.is_ascii_hexdigit());
    if raw_psk || ((8..=63).contains(&value.len()) && printable(value)) {
        Ok(())
    } else {
        Err("A passphrase is 8 to 63 printable ASCII characters")
    }
}
