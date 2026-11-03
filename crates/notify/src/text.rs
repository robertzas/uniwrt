//! Alerts in words, as the UI says them (`web/src/lib/alerts.ts`, `entities.ts`, `metrics.ts`):
//! what an alert is about, what made it fire, levels, durations and times.

use std::{collections::HashMap, fmt::Write as _, time::Duration};

use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    alerts::{AlertCause, AlertRecord, Comparison},
    events::EventLevel,
    metric::{Entity, Metric, Unit},
};

/// What nodes and clients are called: nodes by their names, clients by alias or hostname.
#[derive(Debug, Clone, Default)]
pub struct Names {
    pub nodes: HashMap<NodeId, String>,
    pub clients: HashMap<MacAddr, String>,
}

impl Names {
    fn node(&self, id: NodeId) -> String {
        self.nodes
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("Node {id}"))
    }

    fn client(&self, mac: MacAddr) -> String {
        self.clients
            .get(&mac)
            .cloned()
            .unwrap_or_else(|| mac.to_string())
    }

    /// A link's end: `node:2`, `client:<mac>`, `peer:<name>`.
    fn vertex(&self, vertex: &str) -> String {
        if let Some(id) = vertex.strip_prefix("node:").and_then(|n| n.parse().ok()) {
            return self.node(NodeId::new(id));
        }
        if let Some(mac) = vertex.strip_prefix("client:").and_then(|m| m.parse().ok()) {
            return self.client(mac);
        }
        vertex.strip_prefix("peer:").unwrap_or(vertex).to_owned()
    }

    /// A topology link by its edge id (`node:2~node:3~wifi`): `ap-hall ↔ ap-garden · Wi-Fi`.
    fn link(&self, edge: &str) -> String {
        let mut parts = edge.split('~');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(a), Some(b), Some(medium)) => {
                let medium = match medium {
                    "wan" => "WAN",
                    "ethernet" => "Ethernet",
                    "mesh" => "Mesh",
                    "wifi" => "Wi-Fi",
                    other => other,
                };
                format!("{} ↔ {} · {medium}", self.vertex(a), self.vertex(b))
            }
            _ => edge.to_owned(),
        }
    }
}

/// What an alert is about, in words: a series' entity (`ap-garden · radio1`), else the
/// condition it follows (a WAN interface, a mesh link), else its node, client or the network.
pub fn subject(alert: &AlertRecord, names: &Names) -> String {
    let state = &alert.state;
    if let Ok(entity) = state.subject.parse::<Entity>() {
        return match entity {
            Entity::Site => "Whole network".to_owned(),
            Entity::Node(node) => names.node(node),
            Entity::Device { node, name } | Entity::Radio { node, name } => {
                format!("{} · {name}", names.node(node))
            }
            Entity::Link(edge) => names.link(&edge),
            Entity::Client(mac) => names.client(mac),
        };
    }
    if let Some((_, edge)) = state.subject.split_once("/mesh:") {
        return names.link(edge);
    }
    if let Some(node) = state.node {
        let name = names.node(node);
        return match state.subject.split_once("/wan:") {
            Some((_, device)) => format!("{name} · {device}"),
            None => name,
        };
    }
    match state.client {
        Some(mac) => names.client(mac),
        None => "Whole network".to_owned(),
    }
}

/// What made it fire: `CPU 93 %, above 90 % · worst 97 %`, or how many events matched.
pub fn cause(alert: &AlertRecord) -> Option<String> {
    match &alert.state.cause {
        AlertCause::Event { matches, .. } => {
            (*matches > 1).then(|| format!("{matches} matching events"))
        }
        AlertCause::Metric {
            metric,
            op,
            threshold,
            value,
            worst,
        } => {
            let op = match op {
                Comparison::Above => "above",
                Comparison::Below => "below",
            };
            let mut text = format!(
                "{} {}, {op} {}",
                metric_label(*metric),
                metric_value(*metric, *value),
                metric_value(*metric, *threshold)
            );
            if (worst - value).abs() > f64::EPSILON {
                let _ = write!(text, " · worst {}", metric_value(*metric, *worst));
            }
            Some(text)
        }
    }
}

pub const fn level(level: EventLevel) -> &'static str {
    match level {
        EventLevel::Critical => "Critical",
        EventLevel::Warning => "Warning",
        EventLevel::Notice => "Notice",
        EventLevel::Info => "Info",
    }
}

/// A level as the API names it: `critical`.
pub const fn level_name(level: EventLevel) -> &'static str {
    match level {
        EventLevel::Critical => "critical",
        EventLevel::Warning => "warning",
        EventLevel::Notice => "notice",
        EventLevel::Info => "info",
    }
}

pub const fn metric_label(metric: Metric) -> &'static str {
    match metric {
        Metric::Cpu => "CPU",
        Metric::Memory => "Memory",
        Metric::Load => "Load",
        Metric::Temperature => "Temperature",
        Metric::RxBps => "Received",
        Metric::TxBps => "Sent",
        Metric::Busy => "Channel busy",
        Metric::Noise => "Noise",
        Metric::Stations => "Wi-Fi clients",
        Metric::Signal => "Signal",
        Metric::Snr => "SNR",
        Metric::TxRate => "Down (PHY)",
        Metric::RxRate => "Up (PHY)",
        Metric::DownBps => "Download",
        Metric::UpBps => "Upload",
        Metric::Score => "Experience",
        Metric::Quality => "Link quality",
        Metric::Capacity => "Capacity",
        Metric::Tq => "TQ",
        Metric::Clients => "Clients",
        Metric::NodesOnline => "Nodes online",
    }
}

/// A metric's value with its unit: `92 %`, `-71 dBm`, `48 Mb/s`.
pub fn metric_value(metric: Metric, value: f64) -> String {
    match metric.unit() {
        Unit::Percent => format!("{} %", number(value, 0)),
        Unit::Load => number(value, 2),
        Unit::Celsius => format!("{} °C", number(value, 0)),
        Unit::BitsPerSecond => bitrate(value),
        Unit::Kbps => rate(value),
        Unit::Mbps => rate(value * 1000.0),
        Unit::Dbm => format!("{} dBm", number(value, 0)),
        Unit::Db => format!("{} dB", number(value, 0)),
        Unit::Count => number(value, 1),
        Unit::Score | Unit::Tq => number(value, 0),
    }
}

/// A traffic rate in bits per second: `840 b/s`, `12 kb/s`, `3.4 Mb/s`.
fn bitrate(bps: f64) -> String {
    for (size, unit) in [(1e9, "Gb/s"), (1e6, "Mb/s"), (1e3, "kb/s")] {
        if bps >= size {
            let value = bps / size;
            return format!("{} {unit}", number(value, usize::from(value < 10.0)));
        }
    }
    format!("{} b/s", number(bps, 0))
}

/// A link rate in kbit/s: `866 Mb/s`, `1.2 Gb/s`.
fn rate(kbps: f64) -> String {
    let mbps = kbps / 1000.0;
    if mbps >= 1000.0 {
        return format!("{} Gb/s", number(mbps / 1000.0, 1));
    }
    format!("{} Mb/s", number(mbps, usize::from(mbps < 10.0)))
}

/// At most `digits` decimals, without trailing zeros.
fn number(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    if text == "-0" { "0" } else { text }.to_owned()
}

/// A duration as the UI says it: the two largest units, `12m`, `3h 5m`, `2d 4h`.
pub fn duration(duration: Duration) -> String {
    let s = duration.as_secs();
    let parts = [
        (s / 86_400, "d"),
        (s % 86_400 / 3600, "h"),
        (s % 3600 / 60, "m"),
        (s % 60, "s"),
    ];
    let Some(first) = parts.iter().position(|(value, _)| *value > 0) else {
        return "0s".to_owned();
    };
    parts[first..(first + 2).min(parts.len())]
        .iter()
        .filter(|(value, _)| *value > 0)
        .map(|(value, unit)| format!("{value}{unit}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A time in UTC, to the minute: `2026-11-02 14:02 UTC`.
pub fn time(at: Timestamp) -> String {
    let t = utc(at);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    )
}

/// A time as RFC 3339 in UTC, to the millisecond: `2026-11-02T14:02:00.000Z`.
pub fn rfc3339(at: Timestamp) -> String {
    let t = utc(at);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.millisecond()
    )
}

fn utc(at: Timestamp) -> ::time::OffsetDateTime {
    let nanos = i128::from(at.as_unix_ms()) * 1_000_000;
    ::time::OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .unwrap_or(::time::OffsetDateTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_core::Timestamp;
    use uniwrt_model::metric::Metric;

    use super::{duration, metric_value, rfc3339, time};

    #[test]
    fn values_read_as_in_the_ui() {
        assert_eq!(metric_value(Metric::Cpu, 92.6), "93 %");
        assert_eq!(metric_value(Metric::Load, 1.5), "1.5");
        assert_eq!(metric_value(Metric::Signal, -71.2), "-71 dBm");
        assert_eq!(metric_value(Metric::DownBps, 48_200_000.0), "48 Mb/s");
        assert_eq!(metric_value(Metric::DownBps, 3_420_000.0), "3.4 Mb/s");
        assert_eq!(metric_value(Metric::DownBps, 840.0), "840 b/s");
        assert_eq!(metric_value(Metric::TxRate, 866_700.0), "867 Mb/s");
        assert_eq!(metric_value(Metric::Capacity, 1200.0), "1.2 Gb/s");
        assert_eq!(metric_value(Metric::Stations, 4.0), "4");
    }

    #[test]
    fn durations_and_times() {
        assert_eq!(duration(Duration::ZERO), "0s");
        assert_eq!(duration(Duration::from_secs(12 * 60 + 5)), "12m 5s");
        assert_eq!(duration(Duration::from_mins(185)), "3h 5m");
        assert_eq!(duration(Duration::from_secs(2 * 86_400 + 30)), "2d");
        let at = Timestamp::from_unix_ms(1_793_628_120_123);
        assert_eq!(time(at), "2026-11-02 14:02 UTC");
        assert_eq!(rfc3339(at), "2026-11-02T14:02:00.123Z");
    }
}
