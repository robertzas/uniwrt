//! [`World`]: the simulated house. Nodes stand at fixed places, clients sit or walk between
//! spots, and each second ([`World::step`]) signals drift (indoor path loss plus slow fading),
//! clients join, roam, leave and come back, traffic flows, leases renew and access points
//! sometimes reboot. What the nodes would notice comes back as [`Happening`]s, and the nodes'
//! logd buffers fill as theirs would.
//!
//! The world is a pure function of its seed and the number of steps: it never reads a clock,
//! and its own time starts at the `start` it was built with.

mod bench;
pub(crate) mod cables;
pub(crate) mod catalog;
pub(crate) mod clients;
mod layout;
mod nodes;
pub(crate) mod radio;
mod steering;
mod traffic;

use std::{
    collections::{BTreeMap, VecDeque},
    fmt::Write as _,
    net::{Ipv4Addr, Ipv6Addr},
};

use uniwrt_core::{MacAddr, Timestamp};

pub(crate) use self::{
    bench::{Bench, LendReply, PORT as BENCH_PORT, SEGMENT_ADDRESS},
    layout::FACTORY_LAN,
};
use self::{
    cables::{Cable, Wiring},
    catalog::{KindSpec, Model, Release},
    radio::{Band, Rate},
};
use crate::{SimConfig, rng::Rng};

/// Seconds the world runs before its start, so it starts settled and with some logs.
const WARMUP_S: u64 = 900;

pub(crate) struct World {
    rng: Rng,
    /// Unix ms of the world's first second.
    epoch_ms: i64,
    pub(crate) now_s: u64,
    pub(crate) nodes: Vec<Node>,
    /// Plain cables between nodes ([`cables`]).
    pub(crate) cables: Vec<Cable>,
    pub(crate) clients: Vec<Client>,
    /// The LAN is a /16 (a fleet too big for a /24).
    pub(crate) wide: bool,
    /// Mean seconds between reboots of an access point; `None` for none.
    reboot_mean: Option<f64>,
    next_reboot: u64,
    /// What happened between steps (a ban's kick), for the next step to hand on.
    pending: Vec<Happening>,
    /// The adoption port's device, if the world has one ([`bench`]).
    pub(crate) bench: Option<Bench>,
}

#[expect(clippy::struct_excessive_bools, reason = "a node's traits")]
pub(crate) struct Node {
    pub(crate) name: String,
    pub(crate) gateway: bool,
    pub(crate) model: &'static Model,
    pub(crate) release: &'static Release,
    /// The label MAC; the node's other addresses count up from it ([`Node::mac_at`]).
    pub(crate) mac: MacAddr,
    pub(crate) ip: Ipv4Addr,
    pub(crate) pos: (f64, f64),
    /// Its WAN port carries batman to the gateway (on the gateway: some access point's does).
    pub(crate) wired: bool,
    pub(crate) radios: [Radio; 2],
    pub(crate) booted_ms: i64,
    /// While rebooting: the second it is up again.
    pub(crate) down_until: Option<u64>,
    /// When the gateway last acked the node's own lease.
    pub(crate) lease_ms: i64,
    pub(crate) load: f64,
    pub(crate) cpu: f64,
    pub(crate) memory_used: f64,
    pub(crate) temp: f64,
    /// Counters of the wired devices, by name.
    pub(crate) ports: BTreeMap<String, Counters>,
    pub(crate) logd: Logd,
    /// hostapd's bans: each client kept off until a world ms (`ban` op); forgotten at a reboot.
    pub(crate) banned: BTreeMap<MacAddr, i64>,
    /// usteer runs here with a wpad that has 802.11v, so it answers `get_client_info`
    /// ([`steering`]); the fleet sets it from the node's disk.
    pub(crate) steers: bool,
    /// What its `network` does with its ports ([`cables`]); the fleet sets it from the disk.
    pub(crate) wiring: Wiring,
    /// The factory-default device on the adoption port ([`bench`]).
    pub(crate) bench: bool,
}

impl Node {
    pub(crate) fn up(&self) -> bool {
        self.down_until.is_none()
    }

    /// Up and part of the house's air: the bench device's radios serve no clients and join no
    /// mesh ([`bench`]).
    pub(crate) fn on_air(&self) -> bool {
        self.up() && !self.bench
    }

    /// 0 is the label (and LAN) MAC, 1 the WAN port, 2 and 3 the 2.4 and 5 GHz access points,
    /// 4 the mesh interface, 5 bat0.
    pub(crate) fn mac_at(&self, offset: u8) -> MacAddr {
        let mut octets = self.mac.octets();
        octets[5] = octets[5].wrapping_add(offset);
        MacAddr::new(octets)
    }

    pub(crate) fn bssid(&self, band: Band) -> MacAddr {
        self.mac_at(2 + band.index() as u8)
    }

    pub(crate) fn mesh_mac(&self) -> MacAddr {
        self.mac_at(4)
    }

    /// The address of its wired backhaul port: a DSA port shares the conduit's (the label
    /// MAC), an access point's WAN port has its own.
    pub(crate) fn wired_mac(&self) -> MacAddr {
        if self.gateway {
            self.mac
        } else {
            self.mac_at(1)
        }
    }
}

pub(crate) struct Radio {
    pub(crate) band: Band,
    pub(crate) channel: u8,
    /// dBm.
    pub(crate) txpower: u8,
    pub(crate) noise: f64,
    /// Percent of air time in use.
    pub(crate) busy: f64,
    pub(crate) ap: Counters,
    pub(crate) mesh: Counters,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counters {
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
    pub(crate) rx_packets: u64,
    pub(crate) tx_packets: u64,
}

impl Counters {
    pub(crate) fn add(&mut self, rx: u64, tx: u64) {
        self.rx_bytes += rx;
        self.tx_bytes += tx;
        self.rx_packets += rx.div_ceil(900);
        self.tx_packets += tx.div_ceil(1200);
    }
}

pub(crate) struct Client {
    pub(crate) mac: MacAddr,
    pub(crate) hostname: Option<String>,
    pub(crate) spec: &'static KindSpec,
    pub(crate) ip: Ipv4Addr,
    pub(crate) ip6: Option<Ipv6Addr>,
    pub(crate) pos: (f64, f64),
    target: Option<(f64, f64)>,
    next_move: u64,
    pub(crate) away_until: Option<u64>,
    next_leave: u64,
    /// Slow fading per node and band, in dB.
    fading: Vec<[f64; 2]>,
    pub(crate) assoc: Option<Assoc>,
    /// A wired client's node and port.
    pub(crate) port: Option<(usize, &'static str)>,
    pub(crate) lease_ms: i64,
    busy: bool,
    /// No roaming before this second.
    hold_until: u64,
    /// Roam to this node once at `target` (the opening walk's handover).
    steer: Option<usize>,
    /// After a fast transition the old access point notices the client is gone: (second,
    /// node, band).
    farewell: Option<(u64, usize, Band)>,
}

impl Client {
    pub(crate) fn home(&self) -> bool {
        self.away_until.is_none()
    }

    /// DUID-LL, as odhcpd shows it.
    pub(crate) fn duid(&self) -> String {
        self.mac
            .octets()
            .iter()
            .fold(String::from("00030001"), |mut duid, octet| {
                let _ = write!(duid, "{octet:02x}");
                duid
            })
    }
}

pub(crate) struct Assoc {
    pub(crate) node: usize,
    pub(crate) band: Band,
    pub(crate) since_ms: i64,
    pub(crate) aid: u16,
    pub(crate) signal: f64,
    pub(crate) counters: Counters,
    pub(crate) retries: u64,
    pub(crate) failed: u64,
    pub(crate) inactive_ms: u64,
    pub(crate) tx_rate: Rate,
    pub(crate) rx_rate: Rate,
}

/// Something a node notices, at a time within the step.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Happening {
    pub(crate) node: usize,
    pub(crate) at: Timestamp,
    pub(crate) what: What,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum What {
    /// hostapd's `auth` and `assoc` requests, with the request's signal.
    Auth {
        mac: MacAddr,
        band: Band,
        signal: i16,
    },
    Assoc {
        mac: MacAddr,
        band: Band,
        signal: i16,
    },
    Authorized {
        mac: MacAddr,
        band: Band,
        alg: &'static str,
    },
    Disassoc {
        mac: MacAddr,
        band: Band,
    },
    /// The client answered a BSS transition request: it goes to the BSS `target`.
    Steered {
        mac: MacAddr,
        band: Band,
        target: MacAddr,
    },
    /// dnsmasq acked a lease.
    Lease {
        mac: MacAddr,
        ip: Ipv4Addr,
        name: Option<String>,
    },
    /// The node went down (a reboot); its links drop.
    Down,
    /// The node is up again.
    Up,
    /// A port's link came up or went down (netifd's `link_up`, `link_down`).
    PortLink {
        port: &'static str,
        up: bool,
    },
}

/// A node's log buffer, as logd keeps it: ids count up from boot, the oldest fall out.
#[derive(Default)]
pub(crate) struct Logd {
    pub(crate) records: VecDeque<LogRecord>,
    next_id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LogRecord {
    pub(crate) id: u64,
    /// facility << 3 | severity.
    pub(crate) priority: u8,
    /// 0 for the kernel's messages, which have no tag.
    pub(crate) source: u8,
    pub(crate) time_ms: i64,
    pub(crate) msg: String,
}

impl Logd {
    const CAPACITY: usize = 1000;

    fn starting_at(id: u64) -> Self {
        Self {
            records: VecDeque::new(),
            next_id: id,
        }
    }

    pub(crate) fn push(&mut self, time_ms: i64, priority: u8, msg: String) {
        let (source, msg) = match msg.strip_prefix("kernel: ") {
            Some(text) => (0, text.to_owned()),
            None => (1, msg),
        };
        if self.records.len() == Self::CAPACITY {
            self.records.pop_front();
        }
        self.records.push_back(LogRecord {
            id: self.next_id,
            priority,
            source,
            time_ms,
            msg,
        });
        self.next_id += 1;
    }
}

impl World {
    /// The world `config` describes, settled and running at `start`.
    pub(crate) fn new(config: &SimConfig, start: Timestamp) -> Self {
        let mut world = layout::build(config, start.as_unix_ms() - WARMUP_S as i64 * 1000);
        for _ in 0..WARMUP_S {
            world.step();
        }
        world.opening_walk();
        world
    }

    /// Moves the LAN to where the gateway's address is `gateway`: every node and client keeps
    /// its place in the subnet, as hosts that ask for a lease again do (D-081).
    pub(crate) fn renumber(&mut self, gateway: Ipv4Addr) {
        let shift = u32::from(gateway).wrapping_sub(u32::from(self.nodes[0].ip));
        if shift == 0 {
            return;
        }
        let moved = |ip: Ipv4Addr| Ipv4Addr::from(u32::from(ip).wrapping_add(shift));
        for n in 0..self.nodes.len() {
            if self.on_lan(n) {
                self.nodes[n].ip = moved(self.nodes[n].ip);
            }
        }
        for client in &mut self.clients {
            client.ip = moved(client.ip);
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.epoch_ms + self.now_s as i64 * 1000
    }

    pub(crate) fn now(&self) -> Timestamp {
        Timestamp::from_unix_ms(self.now_ms())
    }

    /// Advances one second.
    pub(crate) fn step(&mut self) -> Vec<Happening> {
        self.now_s += 1;
        let mut out = std::mem::take(&mut self.pending);
        self.step_nodes(&mut out);
        self.step_bench(&mut out);
        self.step_clients(&mut out);
        self.step_air();
        out
    }

    /// The signal `node` hears from a client at `pos` on `band`, without fading.
    pub(crate) fn signal_at(&self, node: usize, band: Band, pos: (f64, f64), offset: f64) -> f64 {
        let node = &self.nodes[node];
        let radio = &node.radios[band.index()];
        f64::from(radio.txpower) + 3.0 + offset - band.path_loss(distance(node.pos, pos))
    }

    /// The signal of the 5 GHz mesh link between two nodes (both antennas count).
    pub(crate) fn mesh_signal(&self, a: usize, b: usize) -> f64 {
        let (a, b) = (&self.nodes[a], &self.nodes[b]);
        f64::from(a.radios[1].txpower) + 6.0 - Band::G5.path_loss(distance(a.pos, b.pos))
    }

    /// The clients associated with `node`.
    pub(crate) fn stations(&self, node: usize) -> impl Iterator<Item = (&Client, &Assoc)> {
        self.clients.iter().filter_map(move |client| {
            client
                .assoc
                .as_ref()
                .filter(|assoc| assoc.node == node)
                .map(|assoc| (client, assoc))
        })
    }

    /// The node a client reaches the network through, if it is connected.
    pub(crate) fn attachment(client: &Client) -> Option<usize> {
        client
            .assoc
            .as_ref()
            .map(|assoc| assoc.node)
            .or(client.port.map(|(node, _)| node))
    }

    fn log(&mut self, node: usize, at_ms: i64, priority: u8, msg: String) {
        self.nodes[node].logd.push(at_ms, priority, msg);
    }
}

pub(crate) fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Mean-reverting drift: one step of an Ornstein–Uhlenbeck process.
fn drift(rng: &mut Rng, value: f64, mean: f64, pull: f64, sigma: f64) -> f64 {
    value + pull * (mean - value) + sigma * rng.normal()
}

#[cfg(test)]
mod tests;
