//! miniupnpd's lease file, which the `leases` set carries: a line per mapping,
//! `<proto>:<external port>:<client>:<internal port>:<expiry>:<description>`, the description
//! possibly holding colons; releases before 2.0 write no expiry. The expiry is 0 for a mapping
//! without a lease time, and otherwise a time on the node's clock (a remaining time where
//! miniupnpd was built to write those).

use std::net::Ipv4Addr;

use uniwrt_model::facts::PortMapping;

/// The node's clock reads at least this many seconds since the epoch: a smaller expiry is a
/// remaining time.
const EPOCH_FLOOR: u64 = 1_000_000_000;

pub(super) fn parse(data: &str, now: u64) -> Vec<PortMapping> {
    data.lines().filter_map(|line| mapping(line, now)).collect()
}

fn mapping(line: &str, now: u64) -> Option<PortMapping> {
    let mut fields = line.trim_end().splitn(5, ':');
    let proto = fields.next()?.to_ascii_lowercase();
    if proto != "tcp" && proto != "udp" {
        return None;
    }
    let external_port = fields.next()?.parse().ok()?;
    let ip: Ipv4Addr = fields.next()?.parse().ok()?;
    let internal_port = fields.next()?.parse().ok()?;
    let rest = fields.next().unwrap_or_default();
    let (expiry, description) = match rest.split_once(':') {
        Some((expiry, description)) => match expiry.parse::<u64>() {
            Ok(expiry) => (Some(expiry), description),
            Err(_) => (None, rest),
        },
        None => (None, rest),
    };
    let expires_in_s = match expiry {
        None | Some(0) => None,
        Some(at) if at >= EPOCH_FLOOR => Some(at.saturating_sub(now)),
        Some(left) => Some(left),
    };
    Some(PortMapping {
        proto,
        external_port,
        ip,
        internal_port,
        description: description.to_owned(),
        expires_in_s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_of_every_release_parse() {
        let now = 1_762_400_000;
        let data = "TCP:32400:10.0.0.20:32400:1762403600:Plex Media Server\n\
                    UDP:3074:10.0.0.21:3074:0:Xbox: Teredo\n\
                    TCP:8080:10.0.0.22:80:old release\n\
                    UDP:5000:10.0.0.23:5000:600:remaining\n\
                    ICMP:1:10.0.0.24:1:0:no\n\
                    TCP:x:10.0.0.25:1:0:bad\n";
        let mappings = parse(data, now);
        let summary: Vec<_> = mappings
            .iter()
            .map(|m| {
                (
                    m.proto.as_str(),
                    m.external_port,
                    m.internal_port,
                    m.description.as_str(),
                    m.expires_in_s,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("tcp", 32400, 32400, "Plex Media Server", Some(3600)),
                ("udp", 3074, 3074, "Xbox: Teredo", None),
                ("tcp", 8080, 80, "old release", None),
                ("udp", 5000, 5000, "remaining", Some(600)),
            ]
        );
        assert_eq!(mappings[0].ip, Ipv4Addr::new(10, 0, 0, 20));
    }
}
