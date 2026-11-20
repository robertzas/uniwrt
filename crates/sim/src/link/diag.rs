//! The diagnostics toolkit's commands on a simulated node (D-141): `ping`, `traceroute`,
//! `nslookup` and `uclient-fetch` as [`uniwrt_node::diag::command`] runs them, answered with
//! output shaped as BusyBox and uclient-fetch print it (captured on OpenWrt 24.10 and 25.12),
//! paced as the real tools pace it. Documentation addresses (RFC 5737) and `.invalid` names
//! never answer.

use std::{net::Ipv4Addr, time::Duration};

use bytes::Bytes;
use uniwrt_node::{
    ExecStream, ExitStatus, Output,
    exec::{STREAM_BUDGET, pipe},
};

/// A command's output: each line after a pause, then its exit code.
struct Script {
    lines: Vec<(Duration, bool, String)>,
    code: u32,
}

impl Script {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            code: 0,
        }
    }

    fn out(&mut self, after_ms: u64, line: impl Into<String>) {
        self.lines
            .push((Duration::from_millis(after_ms), false, line.into()));
    }

    fn err(&mut self, after_ms: u64, line: impl Into<String>) {
        self.lines
            .push((Duration::from_millis(after_ms), true, line.into()));
    }
}

/// Starts `argv` if it is one of the toolkit's commands.
pub(super) fn stream(argv: &[String]) -> Option<ExecStream> {
    let argv = uniwrt_node::diag::tool_argv(argv);
    let script = match argv.first()?.as_str() {
        "ping" => ping(argv),
        "traceroute" => traceroute(argv),
        "nslookup" => nslookup(argv),
        "uclient-fetch" => fetch(argv),
        _ => return None,
    }?;
    let (stream, source) = pipe(format!("`{}`", argv[0]), STREAM_BUDGET);
    tokio::spawn(async move {
        for (pause, err, line) in script.lines {
            tokio::time::sleep(pause).await;
            let data = Bytes::from(format!("{line}\n"));
            let output = if err {
                Output::Stderr(data)
            } else {
                Output::Stdout(data)
            };
            if !source.output.push(output) {
                return;
            }
        }
        source
            .output
            .push(Output::Exit(ExitStatus::Code(script.code)));
    });
    Some(stream)
}

/// Whether `target` never answers: an RFC 5737 address or a `.invalid` name.
fn silent(target: &str) -> bool {
    if let Ok(ip) = target.parse::<Ipv4Addr>() {
        let [a, b, c, _] = ip.octets();
        return matches!((a, b, c), (192, 0, 2) | (198, 51, 100) | (203, 0, 113));
    }
    target.trim_end_matches('.').ends_with(".invalid")
}

/// A stable number from `text`, for addresses and round trips.
fn hash(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

/// The address `target` resolves to: itself, or one made from its name.
fn address(target: &str) -> String {
    if target.parse::<std::net::IpAddr>().is_ok() {
        return target.to_owned();
    }
    let h = hash(target).to_be_bytes();
    format!("93.184.{}.{}", h[0], h[1].max(1))
}

/// The value after `flag` in `argv`.
fn after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let at = argv.iter().position(|arg| arg == flag)?;
    argv.get(at + 1).map(String::as_str)
}

fn ping(argv: &[String]) -> Option<Script> {
    let target = argv.last()?;
    let count: u32 = after(argv, "-c")?.parse().ok()?;
    let ip = address(target);
    let mut script = Script::new();
    script.out(20, format!("PING {target} ({ip}): 56 data bytes"));
    let base = f64::from(hash(target) % 30 + 2);
    let mut times = Vec::new();
    if !silent(target) {
        for seq in 0..count {
            let time = base + f64::from(hash(&format!("{target}{seq}")) % 900) / 100.0;
            times.push(time);
            script.out(
                if seq == 0 { 0 } else { 1000 },
                format!("64 bytes from {ip}: seq={seq} ttl=53 time={time:.3} ms"),
            );
        }
    }
    let received = u32::try_from(times.len()).unwrap_or(u32::MAX);
    let loss = 100 - received * 100 / count.max(1);
    script.out(
        if received == 0 {
            u64::from(count) * 1000
        } else {
            1000
        },
        "",
    );
    script.out(0, format!("--- {target} ping statistics ---"));
    script.out(
        0,
        format!("{count} packets transmitted, {received} packets received, {loss}% packet loss"),
    );
    if times.is_empty() {
        script.code = 1;
    } else {
        let min = times.iter().copied().fold(f64::MAX, f64::min);
        let max = times.iter().copied().fold(0.0, f64::max);
        let avg = times.iter().sum::<f64>() / f64::from(received);
        script.out(
            0,
            format!("round-trip min/avg/max = {min:.3}/{avg:.3}/{max:.3} ms"),
        );
    }
    Some(script)
}

fn traceroute(argv: &[String]) -> Option<Script> {
    let target = argv.last()?;
    let ip = address(target);
    let mut script = Script::new();
    script.out(
        20,
        format!("traceroute to {target} ({ip}), 30 hops max, 46 byte packets"),
    );
    let hops = ["100.64.0.1", "62.115.42.17", "62.115.120.6"];
    for (n, hop) in hops.iter().enumerate() {
        let ms = 1.2 + 4.0 * n as f64;
        script.out(
            250,
            format!(" {}  {hop}  {ms:.3} ms  {:.3} ms", n + 1, ms + 0.4),
        );
    }
    if silent(target) {
        for n in hops.len() + 1..=8 {
            script.out(4000, format!(" {n}  *  *"));
        }
        return Some(script);
    }
    let ms = f64::from(hash(target) % 30 + 14);
    script.out(
        250,
        format!(" {}  {ip}  {ms:.3} ms  {:.3} ms", hops.len() + 1, ms + 0.6),
    );
    Some(script)
}

fn nslookup(argv: &[String]) -> Option<Script> {
    let mut words = argv[1..].iter().filter(|arg| !arg.starts_with("-type="));
    let name = words.next()?;
    let server = words.next().map_or("127.0.0.1", String::as_str);
    let record = argv
        .iter()
        .find_map(|arg| arg.strip_prefix("-type="))
        .unwrap_or("");
    let mut script = Script::new();
    script.out(40, format!("Server:\t\t{server}"));
    script.out(0, format!("Address:\t{server}:53"));
    script.out(0, "");
    if silent(name) {
        script.out(0, format!("** server can't find {name}: NXDOMAIN"));
        script.out(0, "");
        script.code = 1;
        return Some(script);
    }
    script.out(0, "Non-authoritative answer:");
    script.out(0, format!("Name:\t{name}"));
    let h = hash(name);
    if record != "AAAA" {
        script.out(0, format!("Address: {}", address(name)));
    }
    if record.is_empty() || record == "AAAA" {
        if record.is_empty() {
            script.out(0, "");
            script.out(0, "Non-authoritative answer:");
            script.out(0, format!("Name:\t{name}"));
        }
        script.out(0, format!("Address: 2001:db8::{:x}", h & 0xffff));
    }
    script.out(0, "");
    Some(script)
}

fn fetch(argv: &[String]) -> Option<Script> {
    let url = argv.last()?;
    let host = url
        .split("://")
        .nth(1)?
        .split(['/', ':', '?', '#'])
        .next()?;
    let mut script = Script::new();
    script.err(30, format!("Downloading '{url}'"));
    if silent(host) {
        script.err(10_000, "Connection error: Connection timed out");
        script.code = 4;
        return Some(script);
    }
    let port = if url.starts_with("https") { 443 } else { 80 };
    script.err(40, format!("Connecting to {}:{port}", address(host)));
    let bytes = hash(url) % 90_000 + 1_000;
    script.err(
        u64::from(hash(host) % 400 + 80),
        format!("Download completed ({bytes} bytes)"),
    );
    Some(script)
}

#[cfg(test)]
mod tests {
    use uniwrt_node::{ExitStatus, Output};

    use super::stream;

    /// What `argv` prints, and how it exits.
    async fn run(argv: &[&str]) -> (String, ExitStatus) {
        let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let mut stream = stream(&argv).unwrap();
        let mut text = String::new();
        while let Some(output) = stream.next().await {
            match output.unwrap() {
                Output::Stdout(data) | Output::Stderr(data) => {
                    text.push_str(&String::from_utf8_lossy(&data));
                }
                Output::Exit(status) => return (text, status),
            }
        }
        panic!("no exit")
    }

    #[tokio::test(start_paused = true)]
    async fn the_tools_answer_as_on_openwrt() {
        let (text, status) = run(&["ping", "-c", "2", "-w", "7", "1.1.1.1"]).await;
        assert!(text.starts_with(
            "PING 1.1.1.1 (1.1.1.1): 56 data bytes\n64 bytes from 1.1.1.1: seq=0 ttl=53 time="
        ));
        assert!(text.contains("2 packets transmitted, 2 packets received, 0% packet loss"));
        assert_eq!(status, ExitStatus::Code(0));
        let (text, status) = run(&["ping", "-c", "3", "-w", "8", "192.0.2.1"]).await;
        assert!(text.contains("3 packets transmitted, 0 packets received, 100% packet loss"));
        assert_eq!(status, ExitStatus::Code(1));
        let (text, _) = run(&["traceroute", "-n", "openwrt.org"]).await;
        assert!(
            text.lines().nth(4).unwrap().starts_with(" 4  93.184."),
            "{text}"
        );
        let (text, status) = run(&["nslookup", "-type=AAAA", "openwrt.org", "9.9.9.9"]).await;
        assert!(text.starts_with("Server:\t\t9.9.9.9\n"));
        assert!(text.contains("Address: 2001:db8::"));
        assert_eq!(status, ExitStatus::Code(0));
        let (text, status) = run(&["nslookup", "nope.invalid"]).await;
        assert!(text.contains("** server can't find nope.invalid: NXDOMAIN"));
        assert_eq!(status, ExitStatus::Code(1));
        let (text, status) = run(&[
            "uclient-fetch",
            "-O",
            "/dev/null",
            "-T",
            "10",
            "https://openwrt.org/",
        ])
        .await;
        assert!(text.contains("Connecting to 93.184."), "{text}");
        assert_eq!(status, ExitStatus::Code(0));
        assert!(stream(&["sh".to_owned()]).is_none());
    }
}
