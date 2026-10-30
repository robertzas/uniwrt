//! The clients' second: fading, walking, leaving and coming back, choosing an access point
//! (5 GHz while it is good, roaming only once the current one is clearly worse), traffic, and
//! renewing leases at half their time.

use super::{
    Assoc, Counters, Happening, What, World,
    catalog::{ENTRANCE, SPOTS, Standard},
    drift,
    radio::{Band, Rate},
};

/// dnsmasq's lease time; clients renew at half of it.
pub(crate) const LEASE_MS: i64 = 43_200_000;
/// Below this an access point is not worth joining.
const JOIN_FLOOR: f64 = -82.0;
/// Below this a client loses the access point.
const LOSS_FLOOR: f64 = -88.0;
/// A client roams only from a signal below this, to a candidate better by `ROAM_MARGIN`.
const ROAM_BELOW: f64 = -62.0;
const ROAM_MARGIN: f64 = 8.0;
/// A client keeps a fresh access point at least this long.
const ROAM_HOLD_S: u64 = 30;
/// Metres per second a walking client covers.
const WALK: f64 = 1.0;

impl World {
    /// Associates every client with its best access point, as if it had joined a while ago,
    /// without events.
    pub(super) fn settle(&mut self) {
        for c in 0..self.clients.len() {
            if self.clients[c].port.is_some() {
                continue;
            }
            if let Some((node, band, signal)) = self.best(c) {
                let uptime = (self.now_ms() - self.nodes[node].booted_ms) as f64;
                let since = self.now_ms() - self.rng.range(60e3, uptime.min(2.0 * 86_400e3)) as i64;
                let mut assoc = self.new_assoc(c, node, band, signal, since);
                let secs = (self.now_ms() - since) as f64 / 1000.0;
                let down = (secs * self.clients[c].spec.rate * self.rng.range(1.0, 4.0)) as u64;
                assoc.counters.add(down / 10, down);
                self.clients[c].assoc = Some(assoc);
            }
        }
    }

    pub(super) fn step_clients(&mut self, out: &mut Vec<Happening>) {
        for c in 0..self.clients.len() {
            let rng = &mut self.rng;
            for fading in &mut self.clients[c].fading {
                for value in fading {
                    *value = drift(rng, *value, 0.0, 0.02, 0.4);
                }
            }
            if self.clients[c].port.is_some() {
                self.wired_traffic(c);
            } else {
                self.farewell(c, out);
                self.come_and_go(c, out);
                self.walk(c);
                self.associate(c, out);
                self.traffic(c);
            }
            self.renew(c, out);
        }
    }

    /// The signal an access point hears from client `c` now.
    fn client_signal(&self, c: usize, node: usize, band: Band) -> f64 {
        let client = &self.clients[c];
        self.signal_at(node, band, client.pos, client.spec.tx_offset)
            + client.fading[node][band.index()]
    }

    /// The access point and band client `c` would pick now, with its signal.
    fn best(&self, c: usize) -> Option<(usize, Band, f64)> {
        let mut best: Option<(usize, Band, f64)> = None;
        for node in (0..self.nodes.len()).filter(|&node| self.nodes[node].up()) {
            for band in Band::BOTH {
                if band == Band::G5 && !self.clients[c].spec.five_ghz {
                    continue;
                }
                let signal = self.client_signal(c, node, band);
                if signal >= JOIN_FLOOR
                    && best.is_none_or(|(_, b, s)| score(band, signal) > score(b, s))
                {
                    best = Some((node, band, signal));
                }
            }
        }
        best
    }

    fn farewell(&mut self, c: usize, out: &mut Vec<Happening>) {
        let client = &mut self.clients[c];
        if let Some((at, node, band)) = client.farewell
            && self.now_s >= at
        {
            client.farewell = None;
            let mac = client.mac;
            self.disassociated(node, band, mac, 0, out);
        }
    }

    fn come_and_go(&mut self, c: usize, out: &mut Vec<Happening>) {
        let now = self.now_s;
        let client = &mut self.clients[c];
        if let Some(back) = client.away_until {
            if now >= back {
                let spots = client.spec.spots;
                client.away_until = None;
                client.pos = SPOTS[ENTRANCE];
                client.target = Some(SPOTS[spots[self.rng.below(spots.len() as u64) as usize]]);
                client.next_leave = now + self.rng.exp(5_400.0) as u64;
            }
            return;
        }
        if now < client.next_leave {
            return;
        }
        client.away_until = Some(now + self.rng.range(300.0, 2_400.0) as u64);
        client.next_leave = u64::MAX;
        client.target = None;
        if let Some(assoc) = client.assoc.take() {
            let mac = client.mac;
            self.disassociated(assoc.node, assoc.band, mac, 0, out);
        }
    }

    fn walk(&mut self, c: usize) {
        let now = self.now_s;
        let client = &mut self.clients[c];
        if !client.home() {
            return;
        }
        if now >= client.next_move
            && let Some(mean) = client.spec.moves
        {
            let spots = client.spec.spots;
            let spot = SPOTS[spots[self.rng.below(spots.len() as u64) as usize]];
            client.target = Some((
                spot.0 + self.rng.range(-1.5, 1.5),
                spot.1 + self.rng.range(-1.5, 1.5),
            ));
            client.next_move = now + self.rng.exp(mean).max(30.0) as u64;
        }
        if let Some(target) = client.target {
            let left = super::distance(client.pos, target);
            if left <= WALK {
                client.pos = target;
                client.target = None;
            } else {
                let share = WALK / left;
                client.pos.0 += (target.0 - client.pos.0) * share;
                client.pos.1 += (target.1 - client.pos.1) * share;
            }
        }
    }

    fn associate(&mut self, c: usize, out: &mut Vec<Happening>) {
        let now = self.now_s;
        if !self.clients[c].home() || now < self.clients[c].hold_until {
            return;
        }
        let best = self.best(c);
        let Some(current) = self.clients[c].assoc.as_ref().map(|a| (a.node, a.band)) else {
            if let Some((node, band, signal)) = best {
                let alg = if self.clients[c].spec.sae {
                    "sae"
                } else {
                    "open"
                };
                self.connect(c, node, band, signal, alg, 0, out);
                let at = self.now_ms() + 40 + self.rng.below(80) as i64;
                self.ack(c, at, out);
            }
            return;
        };
        let (node, band) = current;
        if !self.nodes[node].up() {
            // The access point vanished: the client notices after a few missed beacons.
            self.clients[c].assoc = None;
            self.clients[c].hold_until = now + 3 + self.rng.below(8);
            return;
        }
        let signal = self.client_signal(c, node, band);
        if signal < LOSS_FLOOR {
            let mac = self.clients[c].mac;
            self.clients[c].assoc = None;
            self.clients[c].hold_until = now + 5;
            self.disassociated(node, band, mac, 0, out);
            return;
        }
        if let Some(assoc) = &mut self.clients[c].assoc {
            assoc.signal = signal;
        }
        let Some((to, to_band, to_signal)) = best else {
            return;
        };
        if (to, to_band) == current
            || signal >= ROAM_BELOW
            || score(to_band, to_signal) < score(band, signal) + ROAM_MARGIN
        {
            return;
        }
        let client = &self.clients[c];
        let (mac, ft, sae) = (client.mac, client.spec.ft && to != node, client.spec.sae);
        if ft {
            self.connect(c, to, to_band, to_signal, "ft", 0, out);
            let at = now + 1 + self.rng.below(3);
            self.clients[c].farewell = Some((at, node, band));
        } else {
            self.disassociated(node, band, mac, 0, out);
            let gap = 300 + self.rng.below(600) as i64;
            self.connect(
                c,
                to,
                to_band,
                to_signal,
                if sae { "sae" } else { "open" },
                gap,
                out,
            );
        }
        self.clients[c].hold_until = now + ROAM_HOLD_S;
    }

    fn new_assoc(&self, c: usize, node: usize, band: Band, signal: f64, since_ms: i64) -> Assoc {
        let (tx_rate, rx_rate) = self.rates(c, node, band, signal);
        let aid = self.stations(node).map(|(_, a)| a.aid).max().unwrap_or(0) + 1;
        Assoc {
            node,
            band,
            since_ms,
            aid,
            signal,
            counters: Counters::default(),
            retries: 0,
            failed: 0,
            inactive_ms: 0,
            tx_rate,
            rx_rate,
        }
    }

    /// The rates to and from client `c`: the downlink at the signal's SNR, the uplink a little
    /// lower, as clients transmit weaker.
    pub(super) fn rates(&self, c: usize, node: usize, band: Band, signal: f64) -> (Rate, Rate) {
        let spec = self.clients[c].spec;
        let width = if band == Band::G2 || spec.standard == Standard::Ht {
            20
        } else {
            band.width_mhz()
        };
        let snr = signal - self.nodes[node].radios[band.index()].noise;
        (
            Rate::for_snr(snr + 3.0, spec.standard, spec.streams, width),
            Rate::for_snr(snr - 2.0, spec.standard, spec.streams, width),
        )
    }

    /// Client `c` joins `node` on `band`, `offset_ms` into the second: hostapd's auth, assoc
    /// and authorized, with the lines it logs.
    #[expect(clippy::too_many_arguments, reason = "one join, spelled out")]
    fn connect(
        &mut self,
        c: usize,
        node: usize,
        band: Band,
        signal: f64,
        alg: &'static str,
        offset_ms: i64,
        out: &mut Vec<Happening>,
    ) {
        let at = self.now_ms() + offset_ms;
        let assoc = self.new_assoc(c, node, band, signal, at);
        let (mac, iface, aid) = (self.clients[c].mac, band.ap_iface(), assoc.aid);
        let dbm = signal.round() as i16;
        for (ms, what) in [
            (
                0,
                What::Auth {
                    mac,
                    band,
                    signal: dbm,
                },
            ),
            (
                3,
                What::Assoc {
                    mac,
                    band,
                    signal: dbm,
                },
            ),
            (6, What::Authorized { mac, band, alg }),
        ] {
            out.push(Happening {
                node,
                at: uniwrt_core::Timestamp::from_unix_ms(at + ms),
                what,
            });
        }
        let mut lines = vec![
            (
                30,
                format!("hostapd: {iface}: STA {mac} IEEE 802.11: authenticated"),
            ),
            (
                30,
                format!("hostapd: {iface}: STA {mac} IEEE 802.11: associated (aid {aid})"),
            ),
            (
                29,
                format!("hostapd: {iface}: AP-STA-CONNECTED {mac} auth_alg={alg}"),
            ),
        ];
        if alg != "ft" {
            lines.push((
                30,
                format!("hostapd: {iface}: STA {mac} WPA: pairwise key handshake completed (RSN)"),
            ));
            lines.push((
                29,
                format!("hostapd: {iface}: EAPOL-4WAY-HS-COMPLETED {mac}"),
            ));
        }
        for (i, (priority, line)) in lines.into_iter().enumerate() {
            self.log(node, at + (i as i64).min(2) * 3, priority, line);
        }
        self.clients[c].assoc = Some(assoc);
    }

    /// hostapd on `node` sees `mac` leave.
    pub(super) fn disassociated(
        &mut self,
        node: usize,
        band: Band,
        mac: uniwrt_core::MacAddr,
        offset_ms: i64,
        out: &mut Vec<Happening>,
    ) {
        if !self.nodes[node].up() {
            return;
        }
        let at = self.now_ms() + offset_ms;
        out.push(Happening {
            node,
            at: uniwrt_core::Timestamp::from_unix_ms(at),
            what: What::Disassoc { mac, band },
        });
        let iface = band.ap_iface();
        self.log(
            node,
            at,
            29,
            format!("hostapd: {iface}: AP-STA-DISCONNECTED {mac}"),
        );
    }

    /// The gateway's dnsmasq acks client `c`'s lease at `at_ms`.
    fn ack(&mut self, c: usize, at_ms: i64, out: &mut Vec<Happening>) {
        let client = &mut self.clients[c];
        client.lease_ms = at_ms;
        let (mac, ip, name) = (client.mac, client.ip, client.hostname.clone());
        self.log(
            0,
            at_ms,
            30,
            format!("dnsmasq-dhcp[2092]: DHCPREQUEST(br-lan) {ip} {mac}"),
        );
        let suffix = name
            .as_deref()
            .map(|name| format!(" {name}"))
            .unwrap_or_default();
        self.log(
            0,
            at_ms + 1,
            30,
            format!("dnsmasq-dhcp[2092]: DHCPACK(br-lan) {ip} {mac}{suffix}"),
        );
        out.push(Happening {
            node: 0,
            at: uniwrt_core::Timestamp::from_unix_ms(at_ms + 1),
            what: What::Lease { mac, ip, name },
        });
    }

    fn renew(&mut self, c: usize, out: &mut Vec<Happening>) {
        let client = &self.clients[c];
        if Self::attachment(client).is_some() && self.now_ms() - client.lease_ms > LEASE_MS / 2 {
            let at = self.now_ms() + self.rng.below(900) as i64;
            self.ack(c, at, out);
        }
    }
}

/// How much a client likes an access point: its signal, plus a bonus for 5 GHz while that is
/// good.
fn score(band: Band, signal: f64) -> f64 {
    signal
        + if band == Band::G5 && signal >= -72.0 {
            8.0
        } else {
            0.0
        }
}
