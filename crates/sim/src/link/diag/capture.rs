//! tcpdump on a simulated node (D-142): `tcpdump -i <device> [-s <snaplen>] -U -w -` as
//! [`uniwrt_node::diag::capture::command`] runs it, once tcpdump is installed: the `.pcap`'s
//! header at once (Ethernet, or Linux cooked v2 on `any`, as tcpdump 4.99 writes them), then a
//! client's DNS lookup and its answer every half second, until the stream is dropped. The filter
//! isn't applied. Devices it doesn't have are refused as tcpdump refuses them.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use uniwrt_node::{
    ExecStream, ExitStatus, Output,
    exec::{STREAM_BUDGET, pipe},
};

use super::{Kit, after};

/// `LINKTYPE_ETHERNET`.
const ETHERNET: u32 = 1;
/// `LINKTYPE_LINUX_SLL2`, what tcpdump writes for `any`.
const SLL2: u32 = 276;
/// How often a lookup happens.
const EVERY: Duration = Duration::from_millis(500);
/// The names looked up, in turn.
const NAMES: [&str; 4] = [
    "openwrt.org",
    "example.com",
    "time.cloudflare.com",
    "github.com",
];
const CLIENT: ([u8; 6], [u8; 4]) = ([0x02, 0, 0, 0, 0x01, 0x64], [192, 168, 1, 100]);
const ROUTER: ([u8; 6], [u8; 4]) = ([0x02, 0, 0, 0, 0, 0x01], [192, 168, 1, 1]);

pub(super) fn stream(argv: &[String], kit: &Kit) -> ExecStream {
    let (stream, source) = pipe("`tcpdump`", STREAM_BUDGET);
    let device = after(argv, "-i").unwrap_or("any").to_owned();
    let snaplen: u32 = after(argv, "-s")
        .and_then(|snaplen| snaplen.parse().ok())
        .unwrap_or(262_144);
    let known = matches!(device.as_str(), "any" | "lo" | "br-lan")
        || kit.ports.iter().any(|(port, _)| *port == device);
    let installed = kit.tcpdump;
    tokio::spawn(async move {
        let say = |text: String| Output::Stderr(Bytes::from(text));
        tokio::time::sleep(Duration::from_millis(20)).await;
        if !installed {
            source
                .output
                .push(say("sh: tcpdump: not found\n".to_owned()));
            source.output.push(Output::Exit(ExitStatus::Code(127)));
            return;
        }
        if !known {
            source.output.push(say(format!(
                "tcpdump: {device}: No such device exists\n(No such device exists)\n"
            )));
            source.output.push(Output::Exit(ExitStatus::Code(1)));
            return;
        }
        let any = device == "any";
        let (link, name) = if any {
            (SLL2, "LINUX_SLL2 (Linux cooked v2)")
        } else {
            (ETHERNET, "EN10MB (Ethernet)")
        };
        source.output.push(say(format!(
            "tcpdump: listening on {device}, link-type {name}, snapshot length {snaplen} bytes\n"
        )));
        source
            .output
            .push(Output::Stdout(Bytes::from(file_header(snaplen, link))));
        for id in 0_u16.. {
            tokio::time::sleep(EVERY).await;
            let name = NAMES[usize::from(id) % NAMES.len()];
            for (packet, inbound) in [(query(id, name), true), (answer(id, name), false)] {
                let frame = framed(&packet, inbound, any);
                if !source
                    .output
                    .push(Output::Stdout(Bytes::from(record(&frame, snaplen))))
                {
                    return;
                }
            }
        }
    });
    stream
}

fn file_header(snaplen: u32, link: u32) -> Vec<u8> {
    let mut header = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0];
    header.extend([0; 8]);
    header.extend(snaplen.to_le_bytes());
    header.extend(link.to_le_bytes());
    header
}

/// `frame` as a pcap record, stamped now and cut at `snaplen`.
fn record(frame: &[u8], snaplen: u32) -> Vec<u8> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let len = u32::try_from(frame.len()).unwrap_or(u32::MAX);
    let kept = len.min(snaplen);
    let mut record = Vec::with_capacity(16 + kept as usize);
    record.extend(
        u32::try_from(now.as_secs())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    record.extend(now.subsec_micros().to_le_bytes());
    record.extend(kept.to_le_bytes());
    record.extend(len.to_le_bytes());
    record.extend_from_slice(&frame[..kept as usize]);
    record
}

/// An IPv4 packet in an Ethernet frame (or a Linux cooked v2 header on `any`), from the client
/// to the router when `inbound`, else back.
fn framed(packet: &[u8], inbound: bool, any: bool) -> Vec<u8> {
    let (from, to) = if inbound {
        (CLIENT, ROUTER)
    } else {
        (ROUTER, CLIENT)
    };
    let mut frame = Vec::with_capacity(20 + packet.len());
    if any {
        frame.extend(0x0800_u16.to_be_bytes());
        frame.extend([0, 0]);
        frame.extend(3_u32.to_be_bytes()); // br-lan's index
        frame.extend(1_u16.to_be_bytes()); // ARPHRD_ETHER
        frame.push(if inbound { 0 } else { 4 }); // PACKET_HOST, PACKET_OUTGOING
        frame.push(6);
        frame.extend(from.0);
        frame.extend([0, 0]);
    } else {
        frame.extend(to.0);
        frame.extend(from.0);
        frame.extend(0x0800_u16.to_be_bytes());
    }
    frame.extend(ipv4_udp(from.1, to.1, inbound, packet));
    frame
}

/// A UDP datagram to or from port 53 (no checksum, as IPv4 allows) in an IPv4 packet.
fn ipv4_udp(from: [u8; 4], to: [u8; 4], to_dns: bool, payload: &[u8]) -> Vec<u8> {
    let (sport, dport) = if to_dns { (40_000, 53) } else { (53, 40_000) };
    let udp_len = u16::try_from(8 + payload.len()).unwrap_or(u16::MAX);
    let mut ip = vec![0x45, 0];
    ip.extend((udp_len + 20).to_be_bytes());
    ip.extend([0, 0, 0x40, 0, 64, 17, 0, 0]);
    ip.extend(from);
    ip.extend(to);
    let sum = checksum(&ip);
    ip[10..12].copy_from_slice(&sum.to_be_bytes());
    ip.extend(u16::to_be_bytes(sport));
    ip.extend(u16::to_be_bytes(dport));
    ip.extend(udp_len.to_be_bytes());
    ip.extend([0, 0]);
    ip.extend_from_slice(payload);
    ip
}

/// The Internet checksum of `header`.
fn checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = header
        .chunks(2)
        .map(|pair| u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !u16::try_from(sum).unwrap_or(u16::MAX)
}

/// A DNS query for `name`'s A record.
fn query(id: u16, name: &str) -> Vec<u8> {
    let mut message = id.to_be_bytes().to_vec();
    message.extend([0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    question(&mut message, name);
    message
}

/// Its answer: one A record.
fn answer(id: u16, name: &str) -> Vec<u8> {
    let mut message = id.to_be_bytes().to_vec();
    message.extend([0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
    question(&mut message, name);
    let h = super::hash(name).to_be_bytes();
    message.extend([
        0xc0,
        0x0c,
        0,
        1,
        0,
        1,
        0,
        0,
        0x01,
        0x2c,
        0,
        4,
        93,
        184,
        h[0],
        h[1].max(1),
    ]);
    message
}

fn question(message: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
        message.push(u8::try_from(label.len()).unwrap_or(63));
        message.extend(label.bytes());
    }
    message.extend([0, 0, 1, 0, 1]);
}

#[cfg(test)]
mod tests {
    use uniwrt_node::{ExitStatus, Output};

    use super::{Kit, checksum, stream};

    fn argv(device: &str) -> Vec<String> {
        ["tcpdump", "-i", device, "-s", "128", "-U", "-w", "-"]
            .map(str::to_owned)
            .to_vec()
    }

    #[tokio::test(start_paused = true)]
    async fn tcpdump_writes_a_pcap_until_dropped() {
        let kit = Kit {
            tcpdump: true,
            ..Kit::default()
        };
        let mut capture = stream(&argv("any"), &kit);
        let Some(Ok(Output::Stderr(listening))) = capture.next().await else {
            panic!("no listening line");
        };
        assert!(listening.starts_with(b"tcpdump: listening on any, link-type LINUX_SLL2"));
        let Some(Ok(Output::Stdout(header))) = capture.next().await else {
            panic!("no header");
        };
        assert_eq!(header[..4], [0xd4, 0xc3, 0xb2, 0xa1]);
        assert_eq!(header[16..], [128, 0, 0, 0, 20, 1, 0, 0]);
        let Some(Ok(Output::Stdout(query))) = capture.next().await else {
            panic!("no packet");
        };
        let len = u32::from_le_bytes(query[8..12].try_into().unwrap()) as usize;
        assert_eq!(query.len(), 16 + len);
        // Linux cooked v2, then an IPv4 header whose checksum adds up.
        assert_eq!(query[16..18], [0x08, 0x00]);
        assert_eq!(checksum(&query[36..56]), 0);
        assert!(query.windows(8).any(|w| w == b"\x07openwrt"));
    }

    #[tokio::test(start_paused = true)]
    async fn tcpdump_refuses_as_on_openwrt() {
        let exit = |mut capture: uniwrt_node::ExecStream| async move {
            let mut text = String::new();
            loop {
                match capture.next().await.unwrap().unwrap() {
                    Output::Stderr(data) => text.push_str(&String::from_utf8_lossy(&data)),
                    Output::Exit(status) => return (text, status),
                    Output::Stdout(_) => panic!("captured"),
                }
            }
        };
        let (text, status) = exit(stream(&argv("lan1"), &Kit::default())).await;
        assert_eq!(
            (text.as_str(), status),
            ("sh: tcpdump: not found\n", ExitStatus::Code(127))
        );
        let kit = Kit {
            tcpdump: true,
            ports: vec![("lan1", true)],
            ..Kit::default()
        };
        let (text, status) = exit(stream(&argv("lan9"), &kit)).await;
        assert!(text.starts_with("tcpdump: lan9: No such device exists\n"));
        assert_eq!(status, ExitStatus::Code(1));
        let mut capture = stream(&argv("lan1"), &kit);
        assert!(matches!(capture.next().await, Some(Ok(Output::Stderr(_)))));
    }
}
