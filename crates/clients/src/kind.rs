//! What kind of device a client is, for its icon: rules over its DHCP hostname, then its
//! vendor (`rules.rs`, one line per rule). DHCP vendor classes and mDNS would sharpen this
//! once collected (features/observability.md, Clients).

use serde::{Deserialize, Serialize};

use crate::rules::{HOSTNAME, Rule, VENDOR};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Phone,
    Tablet,
    Laptop,
    Desktop,
    Server,
    Watch,
    Tv,
    Speaker,
    Console,
    Printer,
    Camera,
    Storage,
    SmartHome,
    Network,
    Unknown,
}

/// The kind the first matching rule gives: hostname rules first, which say more, then the
/// vendor's.
pub fn classify(hostname: Option<&str>, vendor: Option<&str>) -> ClientKind {
    if let Some(hostname) = hostname {
        let name = hostname.to_ascii_lowercase();
        let words: Vec<&str> = name
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        if let Some(rule) = HOSTNAME.iter().find(|rule| matches(rule, &name, &words)) {
            return rule.kind;
        }
    }
    if let Some(vendor) = vendor {
        let vendor = vendor.to_ascii_lowercase();
        let named = |name: &str| {
            vendor
                .strip_prefix(name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        };
        if let Some((_, kind)) = VENDOR.iter().find(|(name, _)| named(name)) {
            return *kind;
        }
    }
    ClientKind::Unknown
}

fn matches(rule: &Rule, name: &str, words: &[&str]) -> bool {
    rule.contains.iter().any(|part| name.contains(part))
        || rule.prefixes.iter().any(|prefix| name.starts_with(prefix))
        || rule.words.iter().any(|word| words.contains(word))
}

#[cfg(test)]
mod tests {
    use super::{ClientKind, classify};

    #[test]
    fn hostnames_say_the_most() {
        for (hostname, kind) in [
            ("Johns-iPhone", ClientKind::Phone),
            ("android-3f2a9c1b2d4e5f60", ClientKind::Phone),
            ("Galaxy-S24", ClientKind::Phone),
            ("SM-G991B", ClientKind::Phone),
            ("Galaxy-Tab-S9", ClientKind::Tablet),
            ("Annas-iPad", ClientKind::Tablet),
            ("DESKTOP-4KQ2M1L", ClientKind::Desktop),
            ("LAPTOP-9F8E7D6C", ClientKind::Laptop),
            ("Toms-MacBook-Pro", ClientKind::Laptop),
            ("Apple-Watch", ClientKind::Watch),
            ("Living-Room-TV", ClientKind::Tv),
            ("Chromecast", ClientKind::Tv),
            ("Kitchen-HomePod", ClientKind::Speaker),
            ("PS5-123", ClientKind::Console),
            ("HP-OfficeJet-Pro", ClientKind::Printer),
            ("front-doorbell", ClientKind::Camera),
            ("diskstation", ClientKind::Storage),
            ("shelly1pm-a1b2c3", ClientKind::SmartHome),
            ("esp32-cam", ClientKind::Camera),
            ("espressif", ClientKind::SmartHome),
            ("raspberrypi", ClientKind::Server),
            ("homeassistant", ClientKind::Server),
            ("OpenWrt", ClientKind::Network),
        ] {
            assert_eq!(classify(Some(hostname), None), kind, "{hostname}");
        }
    }

    #[test]
    fn the_vendor_decides_when_the_hostname_does_not() {
        assert_eq!(classify(None, Some("Espressif")), ClientKind::SmartHome);
        assert_eq!(classify(Some("abc123"), Some("Sonos")), ClientKind::Speaker);
        assert_eq!(classify(None, Some("Raspberry Pi")), ClientKind::Server);
        assert_eq!(
            classify(Some("Johns-iPhone"), Some("Sonos")),
            ClientKind::Phone
        );
        // Apple, Google and Amazon make every kind of device.
        assert_eq!(classify(None, Some("Apple")), ClientKind::Unknown);
        assert_eq!(classify(None, None), ClientKind::Unknown);
    }
}
