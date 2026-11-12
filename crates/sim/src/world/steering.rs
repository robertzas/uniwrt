//! Client steering as usteer does it on a node where it runs ([`Node::steers`], set from the
//! node's disk): it asks an 802.11v client to move with a BSS transition request, and the
//! client agrees, which hostapd pushes as `bss-transition-response`
//! (features/wifi.md, Steering & balancing). Two cases: a roam to another node, which the old
//! access point asks for as the client fades (usteer's roam trigger, which comes before the
//! client's own here) or as the opening walk arrives, and band steering off 2.4 GHz, at
//! Balanced's thresholds whatever the profile. The sim's hostapd takes usteer's requests, as
//! steering c makes the real ones do (D-091).
//!
//! [`Node::steers`]: super::Node::steers

use super::{Band, Happening, What, World};

/// Band steering moves a client heard above this on 2.4 GHz…
const BAND_STEER_ABOVE: f64 = -63.0;
/// …for this long…
const BAND_STEER_AFTER_MS: i64 = 60_000;
/// …to 5 GHz on the same node, when it would hear it there at least this well.
const BAND_STEER_TO: f64 = -72.0;

impl World {
    /// Whether the access points of `node` steer client `c`: usteer runs there and the client
    /// has 802.11v (as for 802.11k, the sim's WPA3 and 802.11r clients).
    fn steers(&self, c: usize, node: usize) -> bool {
        let spec = self.clients[c].spec;
        self.nodes[node].steers && (spec.ft || spec.sae)
    }

    /// The access point `from` asked client `c` to move to `to`, and it agreed: hostapd's
    /// push of its answer, just before it goes. Nothing where usteer doesn't steer it.
    pub(super) fn steered(
        &mut self,
        c: usize,
        (node, band): (usize, Band),
        (to, to_band): (usize, Band),
        out: &mut Vec<Happening>,
    ) {
        if !self.steers(c, node) {
            return;
        }
        let (mac, target) = (self.clients[c].mac, self.nodes[to].bssid(to_band));
        let at = self.now_ms();
        out.push(Happening {
            node,
            at: uniwrt_core::Timestamp::from_unix_ms(at),
            what: What::Steered { mac, band, target },
        });
        let iface = band.ap_iface();
        self.log(
            node,
            at,
            30,
            format!("hostapd: {iface}: BSS-TM-RESP {mac} dialog_token=1 status_code=0 bss_termination_delay=0 target_bssid={target}"),
        );
    }

    /// usteer's band steering: client `c`, heard well on the 2.4 GHz access point of `node`
    /// for a while and well enough on its 5 GHz one, is moved there.
    pub(super) fn band_steer(
        &mut self,
        c: usize,
        node: usize,
        band: Band,
        signal: f64,
        out: &mut Vec<Happening>,
    ) {
        let client = &self.clients[c];
        let Some(assoc) = &client.assoc else {
            return;
        };
        if band != Band::G2
            || !client.spec.five_ghz
            || signal < BAND_STEER_ABOVE
            || self.now_ms() - assoc.since_ms < BAND_STEER_AFTER_MS
            || !self.steers(c, node)
        {
            return;
        }
        let to_signal = self.client_signal(c, node, Band::G5);
        if to_signal < BAND_STEER_TO {
            return;
        }
        self.steered(c, (node, band), (node, Band::G5), out);
        self.roam(c, (node, band), (node, Band::G5, to_signal), out);
    }
}
