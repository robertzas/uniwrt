//! The WAN quality monitor's round and traceroute on the simulated gateway (D-144): answers
//! `probe.uc` as the real script prints, with latencies that wander from round to round (a
//! hash of the round's 30-second slot and each target), and nothing beyond the gateway while
//! its WAN is down.

use std::fmt::Write as _;

use serde_json::{Value, json};
use uniwrt_node::ExecOutput;

use super::commands::ok;

const FIRST_HOP: &str = "100.64.0.1";
const SLOT_MS: i64 = 30_000;

/// A number in 0..1 from `seed` and `key` (FNV-1a, then a mix).
fn noise(seed: i64, key: &str, salt: u32) -> f64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ u64::from(salt);
    for byte in seed.to_le_bytes().iter().chain(key.as_bytes()) {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    #[allow(clippy::cast_precision_loss, reason = "53 bits are plenty for noise")]
    let value = (hash >> 11) as f64 / (1_u64 << 53) as f64;
    value
}

fn rtts(slot: i64, key: &str, base: f64, count: u64) -> Vec<f64> {
    (0..count)
        .filter(|&i| noise(slot, key, 1000 + u32::try_from(i).unwrap_or(0)) > 0.01)
        .map(|i| {
            let jitter = noise(slot, key, u32::try_from(i).unwrap_or(0));
            ((base * (1.0 + 0.25 * jitter)) * 1000.0).round() / 1000.0
        })
        .collect()
}

/// `ucode /var/run/uniwrt/agent/probe.uc <spec>` on the gateway, whose WAN is `up` or not.
pub(crate) fn round(spec: &str, up: bool, now_ms: i64) -> ExecOutput {
    let spec: Value = serde_json::from_str(spec).unwrap_or(Value::Null);
    let slot = now_ms / SLOT_MS;
    let count = spec["count"].as_u64().unwrap_or(10);
    let mut results = Vec::new();
    let lost = |key: &str, sent: u64, error: &str| json!({ "key": key, "sent": sent, "rtt_ms": [], "error": error });
    for target in spec["icmp"].as_array().into_iter().flatten() {
        let key = target["key"].as_str().unwrap_or_default();
        let first_hop = target["host"].is_null();
        if !up {
            results.push(lost(key, count, "sendto: Network unreachable"));
            continue;
        }
        let base = if first_hop {
            1.2
        } else {
            8.0 + 10.0 * noise(0, key, 7)
        };
        results.push(json!({ "key": key, "sent": count, "rtt_ms": rtts(slot, key, base, count), "error": null }));
    }
    for target in spec["dns"].as_array().into_iter().flatten() {
        let key = target["key"].as_str().unwrap_or_default();
        let keys = if target["server"] == "upstreams" {
            vec![format!("{key}@100.64.0.53"), format!("{key}@100.64.0.54")]
        } else {
            vec![key.to_owned()]
        };
        for key in keys {
            results.push(if up {
                json!({ "key": key, "sent": 1, "rtt_ms": [(10.0 + 20.0 * noise(slot, &key, 3)).round()], "error": null })
            } else {
                lost(&key, 1, ";; connection timed out; no servers could be reached")
            });
        }
    }
    for target in spec["http"].as_array().into_iter().flatten() {
        let key = target["key"].as_str().unwrap_or_default();
        results.push(if up {
            json!({ "key": key, "sent": 1, "rtt_ms": [(30.0 + 40.0 * noise(slot, key, 5)).round()], "error": null })
        } else {
            lost(key, 1, "Connection error: Network unreachable")
        });
    }
    let answer = json!({ "route": up, "first_hop": up.then_some(FIRST_HOP), "results": results });
    ok(format!("{answer}\n").into_bytes())
}

/// BusyBox's `traceroute -n … -- <host>` from the gateway.
pub(crate) fn traceroute(host: &str, up: bool) -> ExecOutput {
    let mut text = format!("traceroute to {host} ({host}), 12 hops max, 46 byte packets\n");
    if up {
        let _ = write!(
            text,
            " 1  {FIRST_HOP}  1.2 ms\n 2  100.64.1.1  4.8 ms\n 3  {host}  9.6 ms\n"
        );
    } else {
        text.push_str(" 1  *\n 2  *\n");
    }
    ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{round, traceroute};

    fn answer(up: bool) -> Value {
        let spec = r#"{"count":10,"icmp":[{"key":"first-hop","host":null},{"key":"cloudflare","host":"1.1.1.1"}],"dns":[{"key":"isp-dns","server":"upstreams","name":"openwrt.org"}],"http":[{"key":"web","url":"http://x/"}]}"#;
        serde_json::from_slice(&round(spec, up, 90_000).stdout).unwrap()
    }

    #[test]
    fn an_up_line_answers_every_target() {
        let answer = answer(true);
        assert_eq!(answer["route"], true);
        let results = answer["results"].as_array().unwrap();
        assert_eq!(results.len(), 5);
        assert!(results[1]["rtt_ms"].as_array().unwrap().len() >= 8);
        assert_eq!(results[2]["key"], "isp-dns@100.64.0.53");
        assert!(results.iter().all(|r| r["error"].is_null()));
    }

    #[test]
    fn a_down_line_answers_nothing() {
        let answer = answer(false);
        assert_eq!(answer["route"], false);
        assert!(
            answer["results"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["rtt_ms"].as_array().unwrap().is_empty())
        );
        assert!(String::from_utf8_lossy(&traceroute("1.1.1.1", false).stdout).contains(" 1  *"));
    }
}
