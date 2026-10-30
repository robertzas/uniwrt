//! MAC addresses (EUI-48): lenient parsing, canonical formatting and the address bits that matter.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// An EUI-48 MAC address. It displays and serializes in the canonical lowercase, colon-separated
/// form that OpenWrt tools print (`aa:bb:cc:dd:ee:ff`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(type = "string"))]
pub struct MacAddr([u8; 6]);

/// The input was not six hex octets separated consistently by `:` or `-`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid MAC address {input:?}")]
pub struct ParseMacError {
    input: String,
}

impl MacAddr {
    pub const BROADCAST: Self = Self([0xff; 6]);

    pub const fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    pub const fn octets(self) -> [u8; 6] {
        self.0
    }

    /// The organizationally unique identifier: the first three octets, which name the vendor of
    /// a globally administered address.
    pub const fn oui(self) -> [u8; 3] {
        [self.0[0], self.0[1], self.0[2]]
    }

    /// A group address (multicast or broadcast): the I/G bit of the first octet is set.
    pub const fn is_multicast(self) -> bool {
        self.0[0] & 0x01 != 0
    }

    /// The U/L bit is set. Randomized client MACs take this form, as do the MACs OpenWrt derives
    /// for virtual interfaces, so it marks an address as possibly random, not certainly.
    pub const fn is_locally_administered(self) -> bool {
        self.0[0] & 0x02 != 0
    }
}

impl From<[u8; 6]> for MacAddr {
    fn from(octets: [u8; 6]) -> Self {
        Self(octets)
    }
}

impl FromStr for MacAddr {
    type Err = ParseMacError;

    /// Accepts one or two hex digits per octet in either case, separated by `:` or `-` (one kind
    /// per address), as printed by `ip`, `iw`, `batctl`, busybox and Windows tools.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseMacError {
            input: s.chars().take(32).collect(),
        };
        let separator = if s.contains(':') { ':' } else { '-' };
        let mut octets = [0; 6];
        let mut parts = s.split(separator);
        for octet in &mut octets {
            let part = parts.next().ok_or_else(error)?;
            if part.is_empty() || part.len() > 2 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(error());
            }
            *octet = u8::from_str_radix(part, 16).map_err(|_| error())?;
        }
        if parts.next().is_some() {
            return Err(error());
        }
        Ok(Self(octets))
    }
}

impl fmt::Display for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, octet) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{octet:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for MacAddr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MacAddr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl de::Visitor<'_> for Visitor {
            type Value = MacAddr;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a MAC address such as \"aa:bb:cc:dd:ee:ff\"")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<MacAddr, E> {
                v.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::MacAddr;

    const MAC: MacAddr = MacAddr::new([0x02, 0x1b, 0xc0, 0x0a, 0xff, 0x09]);

    #[test]
    fn parses_the_forms_openwrt_tools_print() {
        for input in [
            "02:1b:c0:0a:ff:09",
            "02:1B:C0:0A:FF:09",
            "02-1b-c0-0a-ff-09",
            "2:1b:c0:a:ff:9",
        ] {
            assert_eq!(input.parse::<MacAddr>(), Ok(MAC), "{input}");
        }
        assert_eq!(MAC.to_string(), "02:1b:c0:0a:ff:09");
        assert_eq!(format!("{MAC:?}"), "02:1b:c0:0a:ff:09");
    }

    #[test]
    fn rejects_malformed_input() {
        for input in [
            "",
            "02:1b:c0:0a:ff",
            "02:1b:c0:0a:ff:09:00",
            "02:1b:c0:0a:ff:",
            "02:1b-c0:0a:ff:09",
            "021bc00aff09",
            "02:1b:c0:0a:ff:0g",
            "02:1b:c0:0a:ff:+9",
            "002:1b:c0:0a:ff:09",
        ] {
            assert!(input.parse::<MacAddr>().is_err(), "{input:?}");
        }
    }

    #[test]
    fn reports_the_address_bits() {
        assert!(MAC.is_locally_administered());
        assert!(!MAC.is_multicast());
        assert!(MacAddr::BROADCAST.is_multicast());
        assert!(!MacAddr::new([0x00, 0x1b, 0xc0, 0, 0, 1]).is_locally_administered());
        assert_eq!(MAC.oui(), [0x02, 0x1b, 0xc0]);
    }

    #[test]
    fn serializes_as_a_canonical_string() {
        assert_eq!(
            serde_json::to_string(&MAC).unwrap(),
            r#""02:1b:c0:0a:ff:09""#
        );
        let parsed: MacAddr = serde_json::from_str(r#""02-1B-C0-0A-FF-09""#).unwrap();
        assert_eq!(parsed, MAC);
        assert!(serde_json::from_str::<MacAddr>(r#""nope""#).is_err());
        assert!(serde_json::from_str::<MacAddr>("42").is_err());
    }

    proptest! {
        #[test]
        fn display_round_trips(octets: [u8; 6]) {
            let mac = MacAddr::new(octets);
            prop_assert_eq!(mac.to_string().parse::<MacAddr>(), Ok(mac));
            prop_assert_eq!(mac.to_string().to_uppercase().replace(':', "-").parse::<MacAddr>(), Ok(mac));
        }

        #[test]
        fn parsing_never_panics(input in ".{0,40}") {
            let _ = input.parse::<MacAddr>();
        }
    }
}
