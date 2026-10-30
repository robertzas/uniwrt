//! Traffic: what each client downloads and uploads per second (bursty, capped by its link),
//! the retries a weak signal costs, and the counters it moves on its access point's radio, its
//! uplink and the gateway's WAN port.

use super::{World, radio::Band};

impl World {
    pub(super) fn traffic(&mut self, c: usize) {
        let Some((node, band, signal)) = self.clients[c]
            .assoc
            .as_ref()
            .map(|a| (a.node, a.band, a.signal))
        else {
            return;
        };
        let jitter = self.rng.normal();
        let (tx_rate, rx_rate) = self.rates(c, node, band, signal + jitter);
        let (down, up) = self.demand(c, f64::from(tx_rate.expected_kbps()) * 125.0);
        let rng = &mut self.rng;
        let client = &mut self.clients[c];
        let Some(assoc) = &mut client.assoc else {
            return;
        };
        let packets_before = assoc.counters.tx_packets;
        assoc.counters.add(up, down);
        let packets = assoc.counters.tx_packets - packets_before;
        let retry_share = ((-signal - 60.0) / 100.0).clamp(0.002, 0.3);
        let retries = (packets as f64 * retry_share * rng.range(0.5, 1.5)) as u64;
        assoc.retries += retries;
        assoc.failed += u64::from(rng.chance(retries as f64 / 200.0));
        assoc.inactive_ms = if client.busy || rng.chance(0.6) {
            rng.below(900)
        } else {
            assoc.inactive_ms + 1000
        };
        assoc.tx_rate = tx_rate;
        assoc.rx_rate = rx_rate;
        self.carry(node, Some(band), up, down);
    }

    pub(super) fn wired_traffic(&mut self, c: usize) {
        let Some((node, port)) = self.clients[c].port else {
            return;
        };
        let (down, up) = self.demand(c, 940e6 / 8.0);
        if let Some(counters) = self.nodes[node].ports.get_mut(port) {
            counters.add(up, down);
        }
        self.carry(node, None, up, down);
    }

    /// Bytes client `c` downloads and uploads this second, at most `cap` bytes.
    fn demand(&mut self, c: usize, cap: f64) -> (u64, u64) {
        let rng = &mut self.rng;
        let client = &mut self.clients[c];
        client.busy = if client.busy {
            !rng.chance(0.08)
        } else {
            rng.chance(0.01)
        };
        let factor = if client.busy { client.spec.burst } else { 1.0 };
        let down = (client.spec.rate * factor * rng.range(0.3, 1.7)).min(cap * 0.8);
        let up = down * rng.range(0.05, 0.2) + 200.0;
        (down as u64, up as u64)
    }

    /// Counts a client's traffic on its access point's radio (if wireless), its uplink, and the
    /// gateway's WAN port.
    fn carry(&mut self, node: usize, band: Option<Band>, up: u64, down: u64) {
        let gateway_wan = self.nodes[0].model.wan_port;
        let at = &mut self.nodes[node];
        if let Some(band) = band {
            at.radios[band.index()].ap.add(up, down);
        }
        if node != 0 {
            if at.wired {
                let wan = at.model.wan_port;
                if let Some(port) = at.ports.get_mut(wan) {
                    port.add(down, up);
                }
            } else {
                at.radios[Band::G5.index()].mesh.add(down, up);
            }
        }
        if let Some(wan) = self.nodes[0].ports.get_mut(gateway_wan) {
            wan.add(down, up);
        }
    }
}
