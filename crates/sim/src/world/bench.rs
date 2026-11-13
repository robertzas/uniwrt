//! The bench (features/adoption.md, Path C): a factory-default access point plugged into the
//! gateway's adoption port, for demo mode's port adoption. It is the world's last node, built
//! from a generator of its own so the rest of the world is drawn as without it; it is plugged
//! in (boots) [`PLUG_S`] after the world starts, and the gateway reports the port's link then.
//!
//! On the port's segment it answers at its `network`'s static LAN addresses (192.168.1.1, as
//! OpenWrt ships); the gateway lends the port to its LAN for a while ([`World::lend`]), and a
//! device whose LAN asks for a lease then takes one from the gateway. It never goes on the air
//! ([`super::Node::on_air`]): its radios serve no clients and join no mesh, wherever it is
//! configured to.

use std::net::Ipv4Addr;

use super::{Happening, What, World};

/// The gateway's port the bench device is plugged into.
pub(crate) const PORT: &str = "lan4";
/// The gateway's address on the port's segment (the site's default).
pub(crate) const SEGMENT_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 250);
/// Seconds after the world's start (its warm-up done) that the device is plugged in.
pub(crate) const PLUG_S: u64 = 20;

/// The adoption port and the device on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bench {
    /// The device's node.
    pub(crate) node: usize,
    /// The gateway lent the port to its LAN.
    pub(crate) lent: Option<Lend>,
    /// The device's LAN asks for a lease (its `network`); the fleet sets it from its disk.
    pub(crate) dhcp: bool,
    /// It holds a lease of the gateway's LAN, at its node's `ip`.
    pub(crate) leased: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lend {
    /// Until this world second.
    Until(u64),
    /// Until the link goes down, which it doesn't on the bench.
    Kept,
}

/// What the lend script answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LendReply {
    Lent,
    Busy,
    Kept,
    Ended,
    /// `keep` of a port that isn't lent.
    NotLent,
}

impl World {
    /// Whether node `n` is the bench device.
    pub(crate) fn is_bench(&self, n: usize) -> bool {
        self.bench.as_ref().is_some_and(|bench| bench.node == n)
    }

    /// Whether node `n` is a host of the gateway's LAN: any node but a bench device without
    /// its lease.
    pub(crate) fn on_lan(&self, n: usize) -> bool {
        self.bench
            .as_ref()
            .is_none_or(|bench| bench.node != n || bench.leased)
    }

    /// Whether the port is lent to the LAN now.
    pub(crate) fn port_lent(&self) -> bool {
        self.bench
            .as_ref()
            .is_some_and(|bench| bench.lent.is_some())
    }

    /// The lend script's `lend`, `keep` and `end` of `port`, for `seconds` (`lend`).
    pub(crate) fn lend(&mut self, verb: &str, port: &str, seconds: u64) -> Option<LendReply> {
        let now = self.now_s;
        let bench = self.bench.as_mut().filter(|_| port == PORT)?;
        Some(match verb {
            "lend" if bench.lent.is_some() => LendReply::Busy,
            "lend" => {
                bench.lent = Some(Lend::Until(now + seconds));
                LendReply::Lent
            }
            "keep" if bench.lent.is_none() => LendReply::NotLent,
            "keep" => {
                bench.lent = Some(Lend::Kept);
                LendReply::Kept
            }
            "end" => {
                bench.lent = None;
                LendReply::Ended
            }
            _ => return None,
        })
    }

    /// The bench's second: the lend runs out, and the device takes (or renews) its lease while
    /// the port is lent and its LAN asks for one.
    pub(super) fn step_bench(&mut self, out: &mut Vec<Happening>) {
        let now = self.now_s;
        let Some(bench) = self.bench.as_mut() else {
            return;
        };
        if matches!(bench.lent, Some(Lend::Until(until)) if now >= until) {
            bench.lent = None;
        }
        let n = bench.node;
        let reaches = bench.lent.is_some() && bench.dhcp && self.nodes[n].up();
        let fresh = reaches && !bench.leased;
        bench.leased = reaches;
        if !reaches {
            if !bench.dhcp {
                self.nodes[n].ip = super::FACTORY_LAN;
            }
            return;
        }
        if fresh {
            // The next host after the access points', as theirs are counted.
            self.nodes[n].ip = Ipv4Addr::from(u32::from(self.nodes[0].ip) + n as u32);
        }
        if fresh || self.lease_due(n) {
            self.node_lease(n, out);
        }
    }

    /// The device is plugged in: the gateway sees the port's link come up.
    pub(super) fn plugged(&self, out: &mut Vec<Happening>) {
        out.push(Happening {
            node: 0,
            at: self.now(),
            what: What::PortLink {
                port: PORT,
                up: true,
            },
        });
    }
}
