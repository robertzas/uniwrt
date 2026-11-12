//! Client steering where the controller asks (features/wifi.md, Steering & balancing; D-093):
//! the agent's `btm` op has the client's access point send a BSS transition request, and an
//! 802.11v client (the sim's WPA3 ones, whose extended capabilities say so) agrees and goes:
//! hostapd pushes its answer as `bss-transition-response`, then the client roams. usteer, where
//! it runs ([`Node::steers`], set from the node's disk), tells the controller where it hears a
//! client (`get_client_info`) and no longer steers on its own: the render turns its band
//! steering and roam triggers off.
//!
//! [`Node::steers`]: super::Node::steers

use serde_json::{Map, Value, json};
use uniwrt_core::MacAddr;

use super::{Band, Happening, What, World};

/// usteer hears probes down to about this.
const HEARD_FLOOR: f64 = -90.0;

/// Why a transition request wasn't sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotSent {
    /// The client isn't connected to the node.
    NotHere,
}

impl World {
    /// The `btm` op on `node`: a transition request to client `mac`, towards the candidates'
    /// BSSIDs (best first), or with none, up a band on the same node. Answers the band of the
    /// access point that sent it. An 802.11v client goes to the first candidate it can join (an
    /// access point up, on a band it has, heard well enough); others ignore the request.
    pub(crate) fn transition(
        &mut self,
        node: usize,
        mac: MacAddr,
        candidates: &[MacAddr],
    ) -> Result<Band, NotSent> {
        let c = self
            .clients
            .iter()
            .position(|client| client.mac == mac)
            .ok_or(NotSent::NotHere)?;
        let band = self.clients[c]
            .assoc
            .as_ref()
            .filter(|a| a.node == node)
            .ok_or(NotSent::NotHere)?
            .band;
        if !self.clients[c].spec.sae {
            return Ok(band);
        }
        let wanted: Vec<(usize, Band)> = if candidates.is_empty() {
            vec![(node, Band::G5)]
        } else {
            candidates
                .iter()
                .filter_map(|bssid| {
                    (0..self.nodes.len()).find_map(|n| {
                        Band::BOTH
                            .into_iter()
                            .find(|&b| self.nodes[n].bssid(b) == *bssid)
                            .map(|b| (n, b))
                    })
                })
                .collect()
        };
        let five = self.clients[c].spec.five_ghz;
        let target = wanted.into_iter().find_map(|(to, to_band)| {
            let usable = (to, to_band) != (node, band)
                && self.nodes[to].up()
                && (to_band == Band::G2 || five);
            let signal = self.client_signal(c, to, to_band);
            (usable && signal >= super::clients::JOIN_FLOOR).then_some((to, to_band, signal))
        });
        if let Some(to) = target {
            let mut out = std::mem::take(&mut self.pending);
            self.steered(c, (node, band), (to.0, to.1), &mut out);
            self.roam(c, (node, band), to, &mut out);
            self.pending = out;
        }
        Ok(band)
    }

    /// The client agreed to go from `node`'s access point to `to`: hostapd's push of its
    /// answer, and the log line hostapd writes, just before it goes.
    fn steered(
        &mut self,
        c: usize,
        (node, band): (usize, Band),
        (to, to_band): (usize, Band),
        out: &mut Vec<Happening>,
    ) {
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

    /// usteer's `get_client_info` on `node`: the client's signal at every access point of the
    /// nodes where usteer runs (`hostapd.<if>` for this node's, `<ip>#hostapd.<if>` for
    /// another's), as far as they hear it; `None` for a client usteer never heard.
    pub(crate) fn usteer_client_info(&self, node: usize, mac: MacAddr) -> Option<Value> {
        let c = self.clients.iter().position(|client| client.mac == mac)?;
        let client = &self.clients[c];
        let mut nodes = Map::new();
        for n in (0..self.nodes.len()).filter(|&n| self.nodes[n].steers && self.nodes[n].up()) {
            for band in Band::BOTH {
                if band == Band::G5 && !client.spec.five_ghz {
                    continue;
                }
                let signal = self.client_signal(c, n, band);
                if signal < HEARD_FLOOR {
                    continue;
                }
                let iface = band.ap_iface();
                let name = if n == node {
                    format!("hostapd.{iface}")
                } else {
                    format!("{}#hostapd.{iface}", self.nodes[n].ip)
                };
                let connected = client
                    .assoc
                    .as_ref()
                    .is_some_and(|a| (a.node, a.band) == (n, band));
                nodes.insert(
                    name,
                    json!({"connected": connected, "signal": signal.round() as i64}),
                );
            }
        }
        (!nodes.is_empty())
            .then(|| json!({"2ghz": true, "5ghz": client.spec.five_ghz, "nodes": nodes}))
    }
}
