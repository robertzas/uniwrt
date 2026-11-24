//! The log lines that name a client, read for the doctor: dnsmasq's DHCP exchanges and
//! hostapd's account of an association. Lines that only repeat what the events say (connected,
//! associated, the handshake done) are left out; lines neither reads are kept as they are.

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;
use uniwrt_model::log::LogRecord;

/// A step of a DHCP exchange as dnsmasq logs it (`DHCPACK(br-lan) 192.168.1.20 <mac> host`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DhcpStep {
    Discover,
    Offer,
    Request,
    Ack,
    /// The server refused the address the client asked for.
    Nak,
    /// The client found the offered address in use.
    Decline,
    Release,
    Inform,
    /// A discover the server had no address for.
    NoAddress,
}

/// What hostapd logged about an association that went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WifiStep {
    /// The key handshake failed: most often a wrong passphrase.
    AuthFailed,
    /// An 802.11r fast transition failed.
    FastTransitionFailed,
    /// The access point dropped the client (deauthenticated or disassociated, with why).
    Dropped,
}

/// How a line reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line {
    Dhcp {
        step: DhcpStep,
        interface: Option<String>,
        ip: Option<String>,
        detail: Option<String>,
    },
    Wifi {
        step: WifiStep,
        interface: Option<String>,
        detail: String,
    },
    /// What the events tell already.
    Routine,
    Other,
}

/// How `record` reads for `mac`; `None` when it doesn't name `mac` (a full-text match on
/// the address's parts elsewhere).
pub(crate) fn read(record: &LogRecord, mac: MacAddr) -> Option<Line> {
    let message = record.entry.message.trim();
    let lower = message.to_ascii_lowercase();
    let mac = mac.to_string();
    if !lower.contains(&mac) {
        return None;
    }
    let program = record.entry.program.as_deref().unwrap_or("");
    if program.starts_with("dnsmasq")
        && let Some(line) = dhcp(message, &mac)
    {
        return Some(line);
    }
    if program.starts_with("hostapd") || program.starts_with("wpa_supplicant") {
        return Some(wifi(message, &lower));
    }
    Some(Line::Other)
}

/// `[<serial> ]DHCP<STEP>(<interface>) [<ip>] <mac> [<detail>]`.
fn dhcp(message: &str, mac: &str) -> Option<Line> {
    let message = message
        .split_once(' ')
        .filter(|(serial, _)| serial.bytes().all(|b| b.is_ascii_digit()))
        .map_or(message, |(_, rest)| rest);
    let rest = message.strip_prefix("DHCP")?;
    let open = rest.find('(')?;
    let close = rest[open..].find(')')? + open;
    let interface = Some(rest[open + 1..close].to_owned()).filter(|name| !name.is_empty());
    let mut words = rest[close + 1..].split_whitespace().peekable();
    let ip = words
        .next_if(|word| !word.eq_ignore_ascii_case(mac))
        .map(str::to_owned);
    if !words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case(mac))
    {
        return None;
    }
    let detail = words.collect::<Vec<_>>().join(" ");
    let detail = (!detail.is_empty()).then_some(detail);
    let step = match &rest[..open] {
        "DISCOVER" if detail.as_deref() == Some("no address available") => DhcpStep::NoAddress,
        "DISCOVER" => DhcpStep::Discover,
        "OFFER" => DhcpStep::Offer,
        "REQUEST" => DhcpStep::Request,
        "ACK" => DhcpStep::Ack,
        "NAK" => DhcpStep::Nak,
        "DECLINE" => DhcpStep::Decline,
        "RELEASE" => DhcpStep::Release,
        "INFORM" => DhcpStep::Inform,
        _ => return None,
    };
    Some(Line::Dhcp {
        step,
        interface,
        ip,
        detail,
    })
}

/// `<interface>: <what>`, as hostapd logs per BSS.
fn wifi(message: &str, lower: &str) -> Line {
    let (interface, text) = match message.split_once(": ") {
        Some((interface, text)) if !interface.contains(' ') => (Some(interface.to_owned()), text),
        _ => (None, message),
    };
    let step = if lower.contains("ft:") && (lower.contains("fail") || lower.contains("invalid")) {
        WifiStep::FastTransitionFailed
    } else if [
        "possible-psk-mismatch",
        "handshake failed",
        "invalid mic",
        "eapol-key timeout",
        "sae: invalid",
        "authentication failed",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        WifiStep::AuthFailed
    } else if lower.contains("deauthenticated due to") || lower.contains("disassociated due to") {
        WifiStep::Dropped
    } else if [
        "ap-sta-connected",
        "ap-sta-disconnected",
        "associated",
        "authenticated",
        "handshake completed",
        "eapol-4way-hs-completed",
        "starting accounting",
        "stopped accounting",
        "assoc_req",
        "binding station",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Line::Routine;
    } else {
        return Line::Other;
    };
    Line::Wifi {
        step,
        interface,
        detail: text.trim().to_owned(),
    }
}
