//! The ISP's line as the speed test meets it (features/wan.md, SQM; D-076): a 500/100 Mbit/s
//! fiber tier whose buffers bloat under load unless the gateway shapes below its rates.

/// The line's rates, in kbit/s: a fiber tier, faster down than up.
pub(crate) const LINE_DOWN_KBPS: u64 = 500_000;
pub(crate) const LINE_UP_KBPS: u64 = 100_000;
/// The round trip to the speed test's host with the line idle, in ms.
const IDLE_MS: f64 = 9.0;
/// How much the ISP's buffers add under load when the gateway doesn't shape: a bloated line.
const BLOAT_DOWN_MS: f64 = 95.0;
const BLOAT_UP_MS: f64 = 240.0;

/// What the speed test's script (`node-agent/speedtest.sh`) prints on the gateway, given what
/// the line carries (`shaped`: SQM's rates when it runs, unless the phase bypasses it) and the
/// gateway's uptime in centiseconds. Shaped below the line, the queue stays on the gateway and
/// short; at the line's rate or unshaped, the ISP's buffers fill.
pub(crate) fn speed_test(
    argv: &[&str],
    up: bool,
    shaped: Option<(u64, u64)>,
    uptime_cs: u64,
) -> String {
    use std::fmt::Write as _;
    let [phase, _device, count, _streams, bypass, ..] = argv else {
        return "{\"err\":\"bad_phase\"}\n".to_owned();
    };
    let count: u64 = count.parse().unwrap_or(10);
    let mut out = String::new();
    if !up {
        return out;
    }
    if *phase == "idle" {
        for i in 0..count {
            let _ = writeln!(out, "{{\"rtt\":{:.3}}}", IDLE_MS + (i % 4) as f64 * 0.15);
        }
        return out;
    }
    let download = *phase == "down";
    let line = if download {
        LINE_DOWN_KBPS
    } else {
        LINE_UP_KBPS
    };
    let shaped = shaped
        .filter(|_| *bypass != "1")
        .map(|(down, up)| if download { down } else { up });
    let (kbps, extra_ms) = match shaped {
        Some(rate) if rate * 100 <= line * 95 => (rate * 97 / 100, 2.0),
        Some(rate) => (
            line.min(rate) * 97 / 100,
            if download { BLOAT_DOWN_MS } else { BLOAT_UP_MS } / 2.0,
        ),
        None => (
            line * 94 / 100,
            if download { BLOAT_DOWN_MS } else { BLOAT_UP_MS },
        ),
    };
    let per_s = kbps * 1000 / 8;
    // Counters as a gateway holds them after some days of traffic.
    let (base_rx, base_tx) = (48_213_000_000_u64, 9_877_000_000_u64);
    for i in 0..=count {
        // TCP's slow start: the first second carries a third.
        let moved = if i == 0 {
            0
        } else {
            per_s / 3 + (i - 1) * per_s
        };
        let (rx, tx) = if download {
            (base_rx + moved, base_tx + moved / 40)
        } else {
            (base_rx + moved / 40, base_tx + moved)
        };
        let _ = writeln!(
            out,
            "{{\"t\":{},\"rx\":{rx},\"tx\":{tx}}}",
            uptime_cs + i * 100
        );
    }
    for i in 0..count {
        let jitter = (i % 5) as f64 * extra_ms / 20.0;
        let _ = writeln!(out, "{{\"rtt\":{:.3}}}", IDLE_MS + extra_ms + jitter);
    }
    out
}

/// SQM's rates as the gateway runs them: its queue on, and the service started.
pub(crate) fn shaped(sqm: Option<&str>, running: bool) -> Option<(u64, u64)> {
    if !running {
        return None;
    }
    let sqm = uniwrt_uci::parse("sqm", sqm?).ok()?;
    let queue = sqm
        .sections_of("queue")
        .find(|q| q.option("enabled") == Some("1"))?;
    Some((
        queue.option("download")?.parse().ok()?,
        queue.option("upload")?.parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{LINE_DOWN_KBPS, shaped, speed_test};

    /// The rate past the first two seconds, in kbit/s, and the largest round trip, in ms.
    fn measured(out: &str, rx: bool) -> (u64, f64) {
        let lines: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let samples: Vec<(u64, u64)> = lines
            .iter()
            .filter(|l| l.get("t").is_some())
            .map(|l| {
                (
                    l["t"].as_u64().unwrap(),
                    l[if rx { "rx" } else { "tx" }].as_u64().unwrap(),
                )
            })
            .collect();
        let (t0, b0) = samples[2];
        let (t1, b1) = *samples.last().unwrap();
        let rtt = lines
            .iter()
            .filter_map(|l| l.get("rtt").and_then(serde_json::Value::as_f64))
            .fold(0.0, f64::max);
        ((b1 - b0) * 8 / ((t1 - t0) * 10), rtt)
    }

    #[test]
    fn the_line_bloats_unless_shaped_below_its_rates() {
        let argv = |phase, bypass| vec![phase, "eth1.201", "12", "4", bypass, "-", "1.1.1.1"];
        let (kbps, rtt) = measured(&speed_test(&argv("down", "0"), true, None, 5000), true);
        assert_eq!(kbps, LINE_DOWN_KBPS * 94 / 100);
        assert!(rtt > 100.0, "{rtt}");
        let tuned = Some((450_000, 95_000));
        let (kbps, rtt) = measured(&speed_test(&argv("down", "0"), true, tuned, 5000), true);
        assert_eq!(kbps, 450_000 * 97 / 100);
        assert!(rtt < 12.0, "{rtt}");
        let (_, rtt) = measured(&speed_test(&argv("up", "1"), true, tuned, 5000), false);
        assert!(rtt > 200.0, "bypassed, the upload bloats: {rtt}");
        assert_eq!(
            speed_test(&argv("idle", "0"), false, None, 0),
            "",
            "the WAN is down"
        );
        let sqm = "config queue 'wan'\n\toption enabled '1'\n\toption download '450000'\n\
                   \toption upload '95000'\n";
        assert_eq!(shaped(Some(sqm), true), tuned);
        assert_eq!(shaped(Some(sqm), false), None, "the service is stopped");
    }
}
