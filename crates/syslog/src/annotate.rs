//! Annotations (observability.md, Logs): `data/log-patterns.toml` maps a regex over an entry's
//! text, for some programs or all, to what it means ([`LogPattern`]). The first pattern in the
//! file that matches wins. The file is embedded and checked by the tests; one pattern is one
//! `[[pattern]]` table.

use std::sync::LazyLock;

use regex::RegexSet;
use serde::Deserialize;
use uniwrt_model::{
    events::EventCategory,
    log::{LogEntry, LogPattern, Severity},
};

const EMBEDDED: &str = include_str!("../data/log-patterns.toml");

/// A pattern file that does not load.
#[derive(Debug, thiserror::Error)]
pub enum PatternError {
    #[error("the patterns are not valid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("pattern {id:?}: {source}")]
    Regex { id: String, source: regex::Error },
    #[error("two patterns are named {0:?}")]
    Duplicate(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    pattern: Vec<Def>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Def {
    id: String,
    #[serde(default)]
    programs: Vec<String>,
    regex: String,
    severity: Severity,
    meaning: String,
    hint: Option<String>,
    link: Option<String>,
    event: Option<EventCategory>,
}

/// Patterns that annotate log entries.
#[derive(Debug)]
pub struct Patterns {
    patterns: Vec<LogPattern>,
    set: RegexSet,
}

impl Patterns {
    /// The embedded `log-patterns.toml`, loaded once.
    pub fn embedded() -> &'static Self {
        static EMBEDDED_PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
            // The tests load the file, so it always parses in a build that passed them.
            Patterns::parse(EMBEDDED).unwrap_or_else(|err| {
                tracing::error!(%err, "the embedded log patterns do not load");
                Patterns::none()
            })
        });
        &EMBEDDED_PATTERNS
    }

    /// No patterns.
    pub fn none() -> Self {
        Self {
            patterns: Vec::new(),
            set: RegexSet::empty(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let file: File = toml::from_str(text)?;
        let mut patterns = Vec::with_capacity(file.pattern.len());
        let mut regexes = Vec::with_capacity(file.pattern.len());
        for def in file.pattern {
            if patterns.iter().any(|p: &LogPattern| p.id == def.id) {
                return Err(PatternError::Duplicate(def.id));
            }
            if let Err(source) = regex::Regex::new(&def.regex) {
                return Err(PatternError::Regex { id: def.id, source });
            }
            regexes.push(def.regex);
            patterns.push(LogPattern {
                id: def.id,
                programs: def.programs,
                severity: def.severity,
                meaning: def.meaning,
                hint: def.hint,
                link: def.link,
                event: def.event,
            });
        }
        let set = RegexSet::new(&regexes).map_err(|source| PatternError::Regex {
            id: "(all)".to_owned(),
            source,
        })?;
        Ok(Self { patterns, set })
    }

    pub fn all(&self) -> &[LogPattern] {
        &self.patterns
    }

    pub fn get(&self, id: &str) -> Option<&LogPattern> {
        self.patterns.iter().find(|pattern| pattern.id == id)
    }

    /// The first pattern that matches the entry.
    pub fn annotate(&self, entry: &LogEntry) -> Option<&LogPattern> {
        if self.patterns.is_empty() {
            return None;
        }
        let program = entry.program.as_deref();
        self.set
            .matches(&entry.message)
            .into_iter()
            .map(|index| &self.patterns[index])
            .find(|pattern| {
                pattern.programs.is_empty()
                    || program.is_some_and(|program| pattern.programs.iter().any(|p| p == program))
            })
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_core::Timestamp;
    use uniwrt_model::log::{LogEntry, Severity};

    use super::{PatternError, Patterns};

    fn annotated(program: &str, message: &str) -> Option<String> {
        let entry = LogEntry {
            seq: None,
            time: Timestamp::from_unix_ms(0),
            facility: 3,
            severity: Severity::Info,
            program: Some(program.to_owned()),
            pid: None,
            message: message.to_owned(),
        };
        Patterns::embedded()
            .annotate(&entry)
            .map(|pattern| pattern.id.clone())
    }

    /// One real line per pattern, each annotated by that pattern and no earlier one.
    const EXAMPLES: [(&str, &str, &str); 22] = [
        (
            "hostapd",
            "phy1-ap0: STA 02:00:00:00:04:00 IEEE 802.11: disassociated due to inactivity",
            "wifi-inactivity",
        ),
        (
            "hostapd",
            "phy1-ap0: STA 02:00:00:00:04:00 IEEE 802.11: deauthenticated due to local deauth request",
            "wifi-local-deauth",
        ),
        (
            "hostapd",
            "phy1-ap0: AP-STA-POSSIBLE-PSK-MISMATCH 02:00:00:00:04:00",
            "wifi-wrong-password",
        ),
        (
            "hostapd",
            "phy1-ap0: STA 02:00:00:00:04:00 WPA: invalid MIC in msg 2/4 of 4-Way Handshake",
            "wifi-wrong-password",
        ),
        (
            "hostapd",
            "phy1-ap0: DFS-RADAR-DETECTED freq=5500 ht_enabled=0 chan_offset=0 chan_width=3 cf1=5530 cf2=0",
            "dfs-radar",
        ),
        (
            "hostapd",
            "phy1-ap0: DFS-CAC-START freq=5500 chan=100 sec_chan=1, width=1, seg0=106, seg1=0, cac_time=60s",
            "dfs-cac",
        ),
        ("netifd", "Network device 'lan2' link is down", "link-down"),
        (
            "netifd",
            "Interface 'wan' has link connectivity loss",
            "interface-link-loss",
        ),
        ("netifd", "Interface 'wan' is now down", "interface-down"),
        (
            "dnsmasq-dhcp",
            "DHCPDISCOVER(br-lan) 02:00:00:00:04:00 no address available",
            "dhcp-pool-full",
        ),
        (
            "kernel",
            "Out of memory: Killed process 1234 (uhttpd) total-vm:4kB",
            "oom",
        ),
        (
            "kernel",
            "mt7915e 0000:01:00.0: Message 000100ed (seq 3) timeout",
            "wifi-firmware-timeout",
        ),
        (
            "kernel",
            "ath10k_pci 0000:01:00.0: firmware crashed! (guid 3f1d)",
            "wifi-firmware-crash",
        ),
        (
            "kernel",
            "br-lan: received packet on lan1 with own address as source address (addr:02:00:00:00:00:01, vlan:0)",
            "bridge-loop",
        ),
        (
            "kernel",
            "nf_conntrack: nf_conntrack: table full, dropping packet",
            "conntrack-full",
        ),
        (
            "kernel",
            "batman_adv: bat0: The MTU of interface mesh0 is too small (1500) to handle the transport of batman-adv packets. Packets going over this interface will be fragmented on layer2 which could impact the performance. Setting the MTU to 1532 would solve the problem.",
            "batman-mtu",
        ),
        (
            "kernel",
            "SQUASHFS error: Unable to read data cache entry [3c0e2]",
            "flash-error",
        ),
        (
            "kernel",
            "uhttpd[1234]: segfault at 0 ip 77d0a4f0 sp 7fb3c6e0 error 6 in libc.so[77c9e000+9a000]",
            "segfault",
        ),
        (
            "dropbear",
            "Bad password attempt for 'root' from 192.0.2.7:51234",
            "ssh-bad-password",
        ),
        ("pppd", "LCP terminated by peer", "ppp-down"),
        (
            "mwan3track",
            "Interface wan (eth1.201) is offline",
            "mwan3-offline",
        ),
        (
            "mwan3track",
            "Interface wanb (usb0) is online",
            "mwan3-online",
        ),
    ];

    #[test]
    fn the_embedded_patterns_load_and_each_has_an_example() {
        let patterns = Patterns::parse(super::EMBEDDED).unwrap();
        assert!(patterns.all().len() >= 15);
        let mut seen = std::collections::BTreeSet::new();
        for (program, message, id) in EXAMPLES {
            assert_eq!(
                annotated(program, message).as_deref(),
                Some(id),
                "{message}"
            );
            seen.insert(id);
        }
        for pattern in patterns.all() {
            assert!(
                seen.contains(pattern.id.as_str()),
                "{} has no example",
                pattern.id
            );
            assert_ne!(pattern.meaning, "");
        }
        // A pattern for some programs does not match the others'.
        assert_eq!(annotated("logger", "Interface 'wan' is now down"), None);
        assert_eq!(
            annotated(
                "hostapd",
                "phy1-ap0: STA 02:00:00:00:04:00 IEEE 802.11: associated"
            ),
            None
        );
    }

    #[test]
    fn broken_files_are_refused() {
        assert!(matches!(
            Patterns::parse("pattern = 1"),
            Err(PatternError::Toml(_))
        ));
        let def = |id: &str, regex: &str| {
            format!(
                "[[pattern]]\nid = \"{id}\"\nregex = '{regex}'\nseverity = \"info\"\nmeaning = \"m\"\n"
            )
        };
        assert!(matches!(
            Patterns::parse(&def("a", "(")),
            Err(PatternError::Regex { .. })
        ));
        let twice = def("a", "x") + &def("a", "y");
        assert!(matches!(
            Patterns::parse(&twice),
            Err(PatternError::Duplicate(_))
        ));
        assert!(Patterns::parse(&(def("a", "x") + "colour = 1\n")).is_err());
        assert!(
            Patterns::none()
                .annotate(&LogEntry {
                    seq: None,
                    time: Timestamp::from_unix_ms(0),
                    facility: 0,
                    severity: Severity::Info,
                    program: None,
                    pid: None,
                    message: "x".into(),
                })
                .is_none()
        );
    }

    #[test]
    fn some_patterns_are_events() {
        use uniwrt_model::events::EventCategory;
        let category = |id: &str| {
            Patterns::embedded()
                .all()
                .iter()
                .find(|pattern| pattern.id == id)
                .and_then(|pattern| pattern.event)
        };
        assert_eq!(category("dfs-radar"), Some(EventCategory::Wifi));
        assert_eq!(category("dhcp-pool-full"), Some(EventCategory::Network));
        assert_eq!(category("ssh-bad-password"), Some(EventCategory::Security));
        assert_eq!(category("wifi-inactivity"), None);
        let def = "[[pattern]]\nid = \"a\"\nregex = 'x'\nseverity = \"info\"\nmeaning = \"m\"\n";
        assert!(Patterns::parse(&format!("{def}event = \"wan\"\n")).is_ok());
        assert!(Patterns::parse(&format!("{def}event = \"weather\"\n")).is_err());
    }
}
