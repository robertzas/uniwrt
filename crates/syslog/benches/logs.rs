//! Log ingest (docs/dev/TESTING.md, Performance; D-149): what the engine's `LogRecorder` does
//! with each line that reaches the controller, by syslog or the agent's follow: `parse` it
//! (RFC 3164, RFC 5424, logd's own timestamps) and `annotate` it with the embedded patterns.
//! The lines are the kinds an OpenWrt fleet sends most: DHCP, hostapd, dnsmasq's query log,
//! netifd, the kernel's bridge and batman-adv, dropbear, usteer.
//!
//! `cargo bench -p uniwrt-syslog` measures; `just profile bench logs` writes flamegraphs.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use uniwrt_core::Timestamp;
use uniwrt_syslog::{Message, Patterns, parse};

const LINES: [&str; 16] = [
    "<30>Nov 26 07:44:13 gw dnsmasq-dhcp[1234]: DHCPDISCOVER(br-lan.10) 02:00:00:00:01:17",
    "<30>Nov 26 07:44:13 gw dnsmasq-dhcp[1234]: DHCPACK(br-lan.10) 192.168.10.23 02:00:00:00:01:17 iPhone",
    "<30>Nov 26 07:44:13 gw dnsmasq[1234]: query[A] time.apple.com from 192.168.10.23",
    "<30>Nov 26 07:44:13 gw dnsmasq[1234]: forwarded time.apple.com to 127.0.0.1#5053",
    "<30>Nov 26 07:44:13 gw dnsmasq[1234]: reply time.apple.com is 17.253.52.253",
    "<30>Nov 26 07:44:13 gw dnsmasq[1234]: config ads.example.net is 0.0.0.0",
    "<30>Nov 26 07:44:14 ap1 hostapd: phy1-ap0: STA 02:00:00:00:01:17 IEEE 802.11: authenticated",
    "<30>Nov 26 07:44:14 ap1 hostapd: phy1-ap0: AP-STA-CONNECTED 02:00:00:00:01:17 auth_alg=ft",
    "<30>Nov 26 07:44:14 ap1 hostapd: phy1-ap0: STA 02:00:00:00:01:17 WPA: pairwise key handshake completed (RSN)",
    "<30>Nov 26 07:44:15 ap2 hostapd: phy0-ap0: AP-STA-DISCONNECTED 02:00:00:00:02:04",
    "<29>Nov 26 07:44:15 gw netifd: Interface 'wan' is now up",
    "<28>Nov 26 07:44:15 gw kernel: [12345.678901] br-lan: port 2(lan2) entered disabled state",
    "<28>Nov 26 07:44:16 ap1 kernel: [  99.120000] batman_adv: bat0: Changing gw mode from: off to: client",
    "<86>Nov 26 07:44:16 gw dropbear[4321]: Password auth succeeded for 'root' from 192.168.1.10:50000",
    "<166>1 2026-11-26T07:44:16.123456+00:00 ap2 usteer 2222 - - 02:00:00:00:02:04: roam scan, signal -78",
    "<30>Nov 26 07:44:17 gw odhcpd[1500]: Using a RA lifetime of 1800 seconds on br-lan",
];

fn received() -> Timestamp {
    Timestamp::from_unix_secs(1_795_000_000)
}

fn logs(c: &mut Criterion) {
    let messages: Vec<Message> = LINES
        .iter()
        .map(|line| parse(line.as_bytes(), received()).expect("every line parses"))
        .collect();
    let patterns = Patterns::embedded();
    let mut group = c.benchmark_group("logs");
    group.throughput(Throughput::Elements(LINES.len() as u64));
    group.bench_function("parse", |b| {
        b.iter(|| {
            for line in LINES {
                black_box(parse(black_box(line.as_bytes()), received()));
            }
        });
    });
    group.bench_function("annotate", |b| {
        b.iter(|| {
            for message in &messages {
                black_box(patterns.annotate(black_box(&message.entry)));
            }
        });
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = uniwrt_bench::criterion();
    targets = logs
}
criterion_main!(benches);
