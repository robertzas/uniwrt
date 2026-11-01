//! [`Sampler`]: averaging what the engine announces into aligned windows, one value per series
//! and window.

use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use uniwrt_clients::ClientSample;
use uniwrt_core::{MacAddr, NodeId, Timestamp};
use uniwrt_model::{
    facts::{DeviceKind, NodeFacts, WifiIfaceMode},
    metric::{Entity, Metric, Resolution, Sample},
    status::NodeState,
};
use uniwrt_topo::{Medium, TopoKind, Topology};

/// The span one raw value stands for; windows start at multiples of it.
pub const WINDOW: Duration = Resolution::Raw.step();
/// How long a window stays open after it ends, for readings that arrive late (a client's
/// sample reaches the sampler after the topology build that carries it).
const GRACE: Duration = Duration::from_secs(5);
/// Counters read further apart than this give no rate: the node was away, and one average
/// over the whole time would read as a burst.
const MAX_INTERVAL: Duration = Duration::from_mins(2);

/// A weighted sum: by time for rates, by reading for gauges.
#[derive(Debug, Clone, Copy, Default)]
struct Sum {
    total: f64,
    weight: f64,
}

#[derive(Debug, Clone, Copy)]
struct Reading {
    at: Timestamp,
    rx_bytes: u64,
    tx_bytes: u64,
}

/// Open windows of every series, and the last counters of every device.
#[derive(Debug, Default)]
pub struct Sampler {
    windows: BTreeMap<i64, BTreeMap<(Entity, Metric), Sum>>,
    /// Windows before this were handed out; what arrives for them is dropped.
    drained_until: i64,
    counters: HashMap<(NodeId, String), Reading>,
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    fn add(&mut self, entity: Entity, metric: Metric, at: Timestamp, value: f64, weight: f64) {
        let window = window_of(at);
        if window < self.drained_until || !value.is_finite() || weight <= 0.0 {
            return;
        }
        let sum = self
            .windows
            .entry(window)
            .or_default()
            .entry((entity, metric))
            .or_default();
        sum.total += value * weight;
        sum.weight += weight;
    }

    fn gauge(&mut self, entity: Entity, metric: Metric, at: Timestamp, value: Option<f64>) {
        if let Some(value) = value {
            self.add(entity, metric, at, value, 1.0);
        }
    }

    /// A node's facts. Gauges are read from every announcement (a set that did not change
    /// still holds its latest values); device counters only when `devices` were collected
    /// anew, since the rate is over the time between two readings.
    pub fn facts(&mut self, node: NodeId, facts: &NodeFacts, devices: bool) {
        let at = facts.collected_at;
        let system = &facts.system;
        let entity = Entity::Node(node);
        self.gauge(
            entity.clone(),
            Metric::Cpu,
            at,
            system.cpu_percent.map(f64::from),
        );
        let memory = &system.memory;
        let used = (memory.total_bytes > 0).then(|| {
            memory.total_bytes.saturating_sub(memory.available_bytes) as f64 * 100.0
                / memory.total_bytes as f64
        });
        self.gauge(entity.clone(), Metric::Memory, at, used);
        self.gauge(
            entity.clone(),
            Metric::Load,
            at,
            Some(f64::from(system.load[0])),
        );
        let temperature = system.temperature_c.map(f64::from);
        self.gauge(entity.clone(), Metric::Temperature, at, temperature);

        let mut node_stations = 0;
        for radio in &facts.radios {
            let aps: Vec<&str> = radio
                .ifaces
                .iter()
                .filter(|iface| iface.mode == WifiIfaceMode::Ap)
                .filter_map(|iface| iface.name.as_deref())
                .collect();
            let stations = facts
                .stations
                .iter()
                .filter(|station| aps.contains(&station.iface.as_str()))
                .count();
            node_stations += stations;
            if !radio.up {
                continue;
            }
            let entity = Entity::Radio {
                node,
                name: radio.name.clone(),
            };
            let busy = radio.channel_busy_percent.map(f64::from);
            self.gauge(entity.clone(), Metric::Busy, at, busy);
            self.gauge(
                entity.clone(),
                Metric::Noise,
                at,
                radio.noise_dbm.map(f64::from),
            );
            self.gauge(entity, Metric::Stations, at, Some(stations as f64));
        }
        if !facts.radios.is_empty() {
            self.gauge(entity, Metric::Stations, at, Some(node_stations as f64));
        }

        if devices {
            for device in &facts.devices {
                if device.kind == DeviceKind::Loopback {
                    continue;
                }
                let now = Reading {
                    at,
                    rx_bytes: device.counters.rx_bytes,
                    tx_bytes: device.counters.tx_bytes,
                };
                let key = (node, device.name.clone());
                let Some(before) = self.counters.insert(key, now) else {
                    continue;
                };
                let elapsed = at.saturating_duration_since(before.at);
                if elapsed.is_zero() || elapsed > MAX_INTERVAL || !device.up {
                    continue;
                }
                let seconds = elapsed.as_secs_f64();
                let entity = Entity::Device {
                    node,
                    name: device.name.clone(),
                };
                for (metric, now, before) in [
                    (Metric::RxBps, now.rx_bytes, before.rx_bytes),
                    (Metric::TxBps, now.tx_bytes, before.tx_bytes),
                ] {
                    if let Some(bytes) = now.checked_sub(before) {
                        let bps = bytes as f64 * 8.0 / seconds;
                        self.add(entity.clone(), metric, at, bps, seconds);
                    }
                }
            }
        }
    }

    /// A topology build: the WAN's traffic, clients and nodes online, and the mesh's links.
    pub fn topology(&mut self, topology: &Topology) {
        let at = topology.at;
        let mut down = None::<f64>;
        let mut up = None::<f64>;
        for edge in &topology.edges {
            match edge.medium {
                Medium::Wan => {
                    if let Some(traffic) = &edge.traffic {
                        if let Some(bps) = traffic.down_bps {
                            *down.get_or_insert(0.0) += bps as f64;
                        }
                        if let Some(bps) = traffic.up_bps {
                            *up.get_or_insert(0.0) += bps as f64;
                        }
                    }
                }
                Medium::Mesh if edge.up => {
                    let entity = Entity::Link(edge.id.clone());
                    for (metric, value) in [
                        (Metric::Signal, edge.signal_dbm.map(f64::from)),
                        (Metric::Tq, edge.tq.map(f64::from)),
                        (Metric::Quality, edge.quality.map(f64::from)),
                        (Metric::Capacity, edge.capacity_mbps.map(f64::from)),
                    ] {
                        self.gauge(entity.clone(), metric, at, value);
                    }
                }
                _ => {}
            }
        }
        let site = Entity::Site;
        self.gauge(site.clone(), Metric::DownBps, at, down);
        self.gauge(site.clone(), Metric::UpBps, at, up);
        let up_ends: Vec<&str> = topology
            .edges
            .iter()
            .filter(|edge| edge.up)
            .flat_map(|edge| [edge.a.as_str(), edge.b.as_str()])
            .collect();
        let clients = topology
            .nodes
            .iter()
            .filter(|vertex| vertex.kind == TopoKind::Client)
            .filter(|vertex| up_ends.contains(&vertex.id.as_str()))
            .count();
        let online = topology
            .nodes
            .iter()
            .filter(|vertex| vertex.node.is_some())
            .filter(|vertex| {
                vertex
                    .state
                    .is_some_and(|state| state != NodeState::Offline)
            })
            .count();
        self.gauge(site.clone(), Metric::Clients, at, Some(clients as f64));
        self.gauge(site, Metric::NodesOnline, at, Some(online as f64));
    }

    /// The clients' new samples.
    pub fn clients(&mut self, samples: &[(MacAddr, ClientSample)]) {
        for (mac, sample) in samples {
            let entity = Entity::Client(*mac);
            let at = sample.at;
            for (metric, value) in [
                (Metric::Signal, sample.signal_dbm.map(f64::from)),
                (Metric::Snr, sample.snr_db.map(f64::from)),
                (Metric::TxRate, sample.tx_kbps.map(f64::from)),
                (Metric::RxRate, sample.rx_kbps.map(f64::from)),
                (Metric::DownBps, sample.down_bps.map(|bps| bps as f64)),
                (Metric::UpBps, sample.up_bps.map(|bps| bps as f64)),
                (Metric::Score, sample.score.map(f64::from)),
            ] {
                self.gauge(entity.clone(), metric, at, value);
            }
        }
    }

    /// Forgets a node's counters, once the engine stops watching it.
    pub fn forget(&mut self, node: NodeId) {
        self.counters.retain(|(of, _), _| *of != node);
    }

    /// The windows that ended (with their grace) by `now`, one sample per series each, oldest
    /// first; later readings for them are dropped.
    pub fn drain(&mut self, now: Timestamp) -> Vec<Sample> {
        let closed = window_of(now.saturating_sub(GRACE));
        let open = self.windows.split_off(&closed);
        let done = std::mem::replace(&mut self.windows, open);
        self.drained_until = self.drained_until.max(closed);
        done.into_iter()
            .flat_map(|(window, series)| {
                series
                    .into_iter()
                    .map(move |((entity, metric), sum)| Sample {
                        entity,
                        metric,
                        at: Timestamp::from_unix_ms(window),
                        value: sum.total / sum.weight,
                    })
            })
            .collect()
    }
}

fn window_ms() -> i64 {
    i64::try_from(WINDOW.as_millis()).unwrap_or(i64::MAX)
}

/// The start of the window `at` falls in.
fn window_of(at: Timestamp) -> i64 {
    at.as_unix_ms().div_euclid(window_ms()) * window_ms()
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
