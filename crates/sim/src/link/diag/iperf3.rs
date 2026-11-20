//! iperf3 on a simulated node (D-142), once its package is installed: the server
//! (`iperf3 -s -1`) says it listens and waits; the client prints what iperf 3.22 prints (captured
//! on OpenWrt 24.10 and 25.12) at the node's uplink rate, a little jitter each second, with
//! `-P` streams, `-R` and `--bidir`. Against the controller (the client's `-c` comes from the
//! session), the host is the controller's address as the sim's `env` names it.

use std::net::Ipv4Addr;

use super::{Kit, Script, after, hash};

/// iperf3's rule around the server's banner.
const RULE: &str = "-----------------------------------------------------------";
/// iperf3's separator between intervals of several streams.
const SEPARATOR: &str = "- - - - - - - - - - - - - - - - - - - - - - - - -";

pub(super) fn script(argv: &[String], kit: &Kit) -> Script {
    let mut script = Script::new();
    if !kit.iperf3 {
        script.err(5, "sh: iperf3: not found");
        script.code = 127;
        return script;
    }
    if argv.iter().any(|arg| arg == "-s") {
        let port = after(argv, "-p").unwrap_or("5201");
        script.out(10, RULE);
        script.out(0, format!("Server listening on {port} (test #1)"));
        script.out(0, RULE);
        // One test, then it exits: the client ends long before.
        script.out(120_000, "");
        return script;
    }
    client(argv, kit, script)
}

/// One of the client's streams.
struct Stream {
    fd: u32,
    /// The client sends on it.
    sends: bool,
    /// `--bidir`'s role column.
    role: Option<&'static str>,
}

impl Stream {
    fn tag(&self) -> String {
        match self.role {
            Some(role) => format!("[{:>3}][{role}]", self.fd),
            None => format!("[{:>3}]", self.fd),
        }
    }
}

/// The client's table: its streams, by role (`--bidir`'s two, or none).
struct Table {
    streams: Vec<Stream>,
    roles: Vec<Option<&'static str>>,
    parallel: u32,
    header: &'static str,
}

impl Table {
    fn new(parallel: u32, reverse: bool, bidir: bool) -> Self {
        let roles = if bidir {
            vec![Some("TX-C"), Some("RX-C")]
        } else {
            vec![None]
        };
        let mut streams = Vec::new();
        for role in &roles {
            for _ in 0..parallel {
                streams.push(Stream {
                    fd: 4 + 2 * u32::try_from(streams.len()).unwrap_or(0),
                    sends: role.map_or(!reverse, |role| role == "TX-C"),
                    role: *role,
                });
            }
        }
        Self {
            streams,
            roles,
            parallel,
            header: if bidir { "[ ID][Role]" } else { "[ ID]" },
        }
    }

    fn sum_tag(role: Option<&str>) -> String {
        role.map_or_else(|| "[SUM]".to_owned(), |role| format!("[SUM][{role}]"))
    }
}

fn client(argv: &[String], kit: &Kit, mut script: Script) -> Script {
    let [a, b, c, _] = kit.ip.octets();
    let controller = Ipv4Addr::new(a, b, c, 50).to_string();
    let host = after(argv, "-c").unwrap_or(&controller).to_owned();
    let port = after(argv, "-p").unwrap_or("5201");
    let seconds: u32 = after(argv, "-t").and_then(|t| t.parse().ok()).unwrap_or(10);
    let parallel: u32 = after(argv, "-P").and_then(|p| p.parse().ok()).unwrap_or(1);
    let reverse = argv.iter().any(|arg| arg == "-R");
    let bidir = argv.iter().any(|arg| arg == "--bidir");
    let table = Table::new(parallel, reverse, bidir);
    script.out(20, format!("Connecting to host {host}, port {port}"));
    if reverse && !bidir {
        script.out(0, format!("Reverse mode, remote host {host} is sending"));
    }
    for (n, stream) in (0_u32..).zip(&table.streams) {
        script.out(
            5,
            format!(
                "[{:>3}] local {} port {} connected to {host} port {port}",
                stream.fd,
                kit.ip,
                40_000 + hash(&host) % 9_000 + n * 2
            ),
        );
    }
    // Each stream's share of the uplink, in Mbit/s; the uplink is shared both ways.
    let share = f64::from(kit.uplink_mbps) / f64::from(parallel) / if bidir { 1.6 } else { 1.0 };
    let totals = intervals(&mut script, &table, share, &host, seconds);
    summary(&mut script, &table, &totals, f64::from(seconds));
    script
}

/// Each second's lines; answers each stream's bytes in all.
fn intervals(script: &mut Script, table: &Table, share: f64, host: &str, seconds: u32) -> Vec<f64> {
    let any_sends = table.streams.iter().any(|stream| stream.sends);
    script.out(
        0,
        format!(
            "{} Interval           Transfer     Bitrate{}",
            table.header,
            if any_sends { "         Retr  Cwnd" } else { "" }
        ),
    );
    let several = table.streams.len() > 1;
    let mut totals = vec![0.0_f64; table.streams.len()];
    for second in 0..seconds {
        let (from, to) = (f64::from(second), f64::from(second + 1));
        let mut sum = vec![0.0_f64; table.roles.len()];
        for (n, stream) in table.streams.iter().enumerate() {
            let jitter = f64::from(hash(&format!("{host}{second}{n}")) % 100) / 1000.0 - 0.05;
            let mbps = share * (1.0 + jitter) * if stream.sends { 1.0 } else { 0.97 };
            let bytes = mbps * 125_000.0;
            totals[n] += bytes;
            let role = table
                .roles
                .iter()
                .position(|role| *role == stream.role)
                .unwrap_or(0);
            sum[role] += bytes;
            let pause = if n == 0 { 1000 } else { 0 };
            script.out(
                pause,
                interval(&stream.tag(), from, to, bytes, stream.sends, None),
            );
        }
        if table.parallel > 1 {
            for (role, bytes) in table.roles.iter().zip(&sum) {
                script.out(
                    0,
                    interval(&Table::sum_tag(*role), from, to, *bytes, false, None),
                );
            }
        }
        if several {
            script.out(0, SEPARATOR);
        }
    }
    if !several {
        script.out(0, SEPARATOR);
    }
    totals
}

/// The totals: each stream's as its sender and receiver, each role's sum, and the end.
fn summary(script: &mut Script, table: &Table, totals: &[f64], end: f64) {
    script.out(
        0,
        format!(
            "{} Interval           Transfer     Bitrate         Retr",
            table.header
        ),
    );
    for (stream, bytes) in table.streams.iter().zip(totals) {
        let tag = stream.tag();
        script.out(0, interval(&tag, 0.0, end, *bytes, false, Some(true)));
        script.out(
            0,
            interval(&tag, 0.0, end, bytes * 0.9998, false, Some(false)),
        );
    }
    if table.parallel > 1 {
        for role in &table.roles {
            let bytes: f64 = table
                .streams
                .iter()
                .zip(totals)
                .filter(|(stream, _)| stream.role == *role)
                .map(|(_, bytes)| bytes)
                .sum();
            let tag = Table::sum_tag(*role);
            script.out(0, interval(&tag, 0.0, end, bytes, false, Some(true)));
            script.out(
                0,
                interval(&tag, 0.0, end, bytes * 0.9998, false, Some(false)),
            );
        }
    }
    script.out(0, "");
    script.out(0, "iperf Done.");
}

/// One line of the table: an interval's (`total` none), or a total's as its `sender` or
/// receiver; a sending interval shows its retransmits and window.
fn interval(tag: &str, from: f64, to: f64, bytes: f64, sends: bool, total: Option<bool>) -> String {
    let seconds = (to - from).max(0.01);
    let mbps = bytes * 8.0 / seconds / 1e6;
    let times = format!("{from:6.2}-{to:<6.2}");
    let tail = match total {
        Some(true) => "    0            sender".to_owned(),
        Some(false) => "                  receiver".to_owned(),
        None if sends => "    0   2.25 MBytes       ".to_owned(),
        None => "                  ".to_owned(),
    };
    format!(
        "{tag} {times} sec  {}  {} Mbits/sec{tail}",
        transfer(bytes),
        figure(mbps)
    )
}

/// Bytes as iperf3 prints them, in binary units.
fn transfer(bytes: f64) -> String {
    let (value, unit) = if bytes >= 1024.0 * 1024.0 * 1024.0 {
        (bytes / 1024.0 / 1024.0 / 1024.0, "GBytes")
    } else if bytes >= 1024.0 * 1024.0 {
        (bytes / 1024.0 / 1024.0, "MBytes")
    } else {
        (bytes / 1024.0, "KBytes")
    };
    format!("{} {unit}", figure(value))
}

/// A figure as iperf3's `unit_snprintf` prints it: 3 significant digits, 4 wide.
fn figure(value: f64) -> String {
    if value >= 99.95 {
        format!("{value:4.0}")
    } else if value >= 9.995 {
        format!("{value:4.1}")
    } else {
        format!("{value:4.2}")
    }
}
