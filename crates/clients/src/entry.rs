//! One client in the [`Book`](crate::Book): its record, where it is (its open session and the
//! one before), its counters, recent samples and roams, and the view made of them.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt::Write as _,
    net::IpAddr,
    sync::Arc,
    time::Duration,
};

use uniwrt_core::{MacAddr, Timestamp};
use uniwrt_model::{
    client::{ClientRecord, ClientSession, ClientWifi, SessionEnd},
    wifi::ChannelWidth,
};

use crate::{
    BookConfig, Changes, Client, ClientKind, ClientSample, Connection, Experience, classify, oui,
    score::{Inputs, experience, max_rate_kbps},
    view::Seen,
};

/// How long roams count toward flapping.
const ROAMS_KEPT: Duration = Duration::from_hours(1);
/// Frames sent between two collects that make a retry rate worth computing.
const MIN_FRAMES: u64 = 20;
/// Traffic (both ways) above which a client is moving data and its rates mean something.
const ACTIVE_BPS: u64 = 8_000;
/// Hostnames too common to tie randomized addresses together.
const GENERIC: &[&str] = &[
    "iphone",
    "ipad",
    "android",
    "localhost",
    "unknown",
    "macbook",
    "macbook-pro",
    "macbook-air",
    "imac",
    "galaxy",
    "pixel",
    "phone",
    "laptop",
    "desktop",
    "watch",
    "espressif",
];

#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) record: ClientRecord,
    vendor: Option<&'static str>,
    kind: ClientKind,
    client_id: Option<String>,
    ips: Vec<IpAddr>,
    pub(crate) online: bool,
    connection: Option<Connection>,
    experience: Option<Experience>,
    pub(crate) session: Option<ClientSession>,
    /// The session before, which may go on (the same association, seen again) or turn out a
    /// roam (the client shows up elsewhere soon after).
    pub(crate) last: Option<ClientSession>,
    pub(crate) samples: VecDeque<ClientSample>,
    counters: Option<Counters>,
    /// Retries and failures per frame sent, in percent, over the last collects that sent
    /// enough.
    retries: Option<(f32, f32)>,
    roams: VecDeque<Timestamp>,
    /// The `last_seen` saved last.
    pub(crate) saved: Timestamp,
    pub(crate) view: Arc<Client>,
}

#[derive(Debug, Clone, Copy)]
struct Counters {
    at: Timestamp,
    packets: u64,
    retries: Option<u64>,
    failed: Option<u64>,
}

impl Entry {
    pub(crate) fn new(record: ClientRecord) -> Self {
        let vendor = oui::vendor(record.mac);
        let kind = classify(record.hostname.as_deref(), vendor);
        let saved = record.last_seen;
        let mut entry = Self {
            record,
            vendor,
            kind,
            client_id: None,
            ips: Vec::new(),
            online: false,
            connection: None,
            experience: None,
            session: None,
            last: None,
            samples: VecDeque::new(),
            counters: None,
            retries: None,
            roams: VecDeque::new(),
            saved,
            view: Arc::new(placeholder()),
        };
        entry.view = Arc::new(entry.view(None));
        entry
    }

    pub(crate) fn mac(&self) -> MacAddr {
        self.record.mac
    }

    /// Notes who the client says it is: its hostname, addresses and DHCP client ID; true when
    /// the hostname changed (to save).
    fn identify(&mut self, seen: &Seen<'_>) -> bool {
        let hostname = seen.vertex.name.clone();
        let renamed = hostname.is_some() && hostname != self.record.hostname;
        if renamed {
            self.record.hostname = hostname;
            self.kind = classify(self.record.hostname.as_deref(), self.vendor);
        }
        if !seen.vertex.ips.is_empty() {
            self.ips.clone_from(&seen.vertex.ips);
        }
        if let Some(id) = seen.client_id {
            self.client_id = Some(id.to_owned());
        }
        renamed
    }

    /// The client is connected where `seen` says.
    pub(crate) fn online(&mut self, seen: &Seen<'_>, config: &BookConfig, changes: &mut Changes) {
        let renamed = self.identify(seen);
        if seen.at > self.record.last_seen {
            self.record.last_seen = seen.at;
        }
        let start = seen.station.map_or(seen.at, |station| {
            seen.at
                .saturating_sub(Duration::from_secs(station.connected_s))
        });
        let here = ClientSession {
            mac: self.mac(),
            node: seen.node,
            port: seen.port.clone(),
            medium: seen.medium,
            band: seen.edge.band,
            ssid: seen.edge.ssid.clone(),
            start,
            end: None,
            ended_by: None,
        };
        self.place(here, seen, config, changes);
        self.online = true;
        let learned = seen
            .station
            .is_some_and(|station| self.record.wifi.learn(station, seen.edge.band));
        if renamed
            || learned
            || self.record.last_seen.saturating_duration_since(self.saved) >= config.save_every
        {
            self.save(changes);
        }
        self.measure(seen, config, changes);
    }

    /// Starts, continues or moves the client's session to `here`.
    fn place(
        &mut self,
        mut here: ClientSession,
        seen: &Seen<'_>,
        config: &BookConfig,
        changes: &mut Changes,
    ) {
        if let Some(open) = &self.session {
            if open.same_place(&here) {
                return;
            }
            // It moved while connected: it roamed.
            let end = here.start.max(open.start);
            self.close(end, SessionEnd::Roamed, changes);
            self.roams.push_back(end);
        } else if let Some(last) = &mut self.last
            && let Some(end) = last.end
        {
            // An association that began before the session ended is the same one: the node
            // stopped reporting for a while, or the controller restarted.
            let continued = if seen.station.is_some() {
                here.start <= end
            } else {
                seen.at.saturating_duration_since(end) <= config.wired_gap
            };
            if continued && last.same_place(&here) {
                last.end = None;
                last.ended_by = None;
                changes.sessions.push(last.clone());
                self.session = self.last.take();
                return;
            }
            if !last.same_place(&here)
                && last.ended_by == Some(SessionEnd::Left)
                && here.start.saturating_duration_since(end) <= config.roam_window
            {
                last.ended_by = Some(SessionEnd::Roamed);
                changes.sessions.push(last.clone());
                self.roams.push_back(end);
            }
        }
        if let Some(end) = self.last.as_ref().and_then(|last| last.end) {
            here.start = here.start.max(end);
        }
        changes.sessions.push(here.clone());
        self.session = Some(here);
    }

    /// The client is gone, or its node stopped reporting: its session ends when it was last
    /// seen.
    pub(crate) fn offline(&mut self, changes: &mut Changes) {
        if !self.online && self.session.is_none() {
            return;
        }
        self.online = false;
        self.experience = None;
        self.counters = None;
        let end = self.record.last_seen;
        if self.session.is_some() {
            self.close(end, SessionEnd::Left, changes);
        }
        if self.record.last_seen > self.saved {
            self.save(changes);
        }
    }

    fn close(&mut self, end: Timestamp, by: SessionEnd, changes: &mut Changes) {
        if let Some(mut session) = self.session.take() {
            session.end = Some(end.max(session.start));
            session.ended_by = Some(by);
            changes.sessions.push(session.clone());
            self.last = Some(session);
        }
    }

    pub(crate) fn save(&mut self, changes: &mut Changes) {
        self.saved = self.record.last_seen;
        changes.records.push(self.record.clone());
    }

    /// The connection as `seen` shows it, the score and a sample when one is due.
    fn measure(&mut self, seen: &Seen<'_>, config: &BookConfig, changes: &mut Changes) {
        let (station, radio) = (seen.station, seen.radio);
        let traffic = seen.edge.traffic;
        self.connection = Some(Connection {
            node: seen.node,
            upstream: seen.upstream.to_owned(),
            upstream_name: seen.upstream_name.map(str::to_owned),
            port: seen.port.clone(),
            medium: seen.medium,
            since: self
                .session
                .as_ref()
                .map_or(seen.at, |session| session.start),
            band: seen.edge.band,
            ssid: seen.edge.ssid.clone(),
            channel: radio.and_then(|radio| radio.channel),
            signal_dbm: station
                .and_then(|station| station.signal_dbm)
                .or(seen.edge.signal_dbm),
            noise_dbm: radio.and_then(|radio| radio.noise_dbm),
            rx_rate: station.and_then(|station| station.rx_rate),
            tx_rate: station.and_then(|station| station.tx_rate),
            standard: station.and_then(|station| station.standard),
            capabilities: station.map_or_else(Vec::new, |station| {
                station.capabilities.iter().copied().collect()
            }),
            capacity_mbps: seen.edge.capacity_mbps,
            traffic,
        });
        let Some(station) = station else {
            self.experience = None;
            return;
        };
        self.count(seen.at, station);
        while self
            .roams
            .front()
            .is_some_and(|at| seen.at.saturating_duration_since(*at) > ROAMS_KEPT)
        {
            self.roams.pop_front();
        }
        let rates = [station.tx_rate, station.rx_rate];
        let best = rates.iter().flatten().max_by_key(|rate| rate.kbps);
        let standard = match (station.standard, radio.and_then(|radio| radio.standard)) {
            (Some(client), Some(radio)) => Some(client.min(radio)),
            (client, radio) => client.or(radio).or(best.map(|rate| rate.standard)),
        };
        let width = radio
            .and_then(|radio| radio.width)
            .or(best.and_then(|rate| rate.width))
            .unwrap_or(ChannelWidth::Mhz20);
        let streams = rates.iter().flatten().filter_map(|rate| rate.nss).max();
        let moved = traffic
            .and_then(|traffic| Some(traffic.down_bps? + traffic.up_bps?))
            .unwrap_or(0);
        let inputs = Inputs {
            signal_dbm: station.signal_dbm,
            noise_dbm: radio.and_then(|radio| radio.noise_dbm),
            rate_kbps: best.map(|rate| rate.kbps),
            capability_kbps: standard
                .map(|standard| max_rate_kbps(standard, width, streams.unwrap_or(1))),
            active: moved >= ACTIVE_BPS,
            retry_percent: self.retries.map(|(retries, _)| retries),
            failed_percent: self.retries.map(|(_, failed)| failed),
            busy_percent: radio.and_then(|radio| radio.channel_busy_percent),
            roams_last_hour: u32::try_from(self.roams.len()).ok(),
        };
        self.experience = experience(&inputs);
        let due = self
            .samples
            .back()
            .is_none_or(|last| seen.at.saturating_duration_since(last.at) >= config.sample_every);
        if due && self.samples.back().is_none_or(|last| seen.at > last.at) {
            let sample = ClientSample {
                at: seen.at,
                signal_dbm: station.signal_dbm,
                snr_db: inputs.signal_dbm.zip(inputs.noise_dbm).map(|(s, n)| s - n),
                tx_kbps: station.tx_rate.map(|rate| rate.kbps),
                rx_kbps: station.rx_rate.map(|rate| rate.kbps),
                down_bps: traffic.and_then(|traffic| traffic.down_bps),
                up_bps: traffic.and_then(|traffic| traffic.up_bps),
                score: self.experience.as_ref().map(|experience| experience.score),
            };
            self.samples.push_back(sample);
            while self.samples.front().is_some_and(|first| {
                seen.at.saturating_duration_since(first.at) > config.samples_kept
            }) {
                self.samples.pop_front();
            }
            changes.samples.push((self.mac(), sample));
        }
    }

    /// The retry and failure rates since the previous collect, from the station's counters.
    fn count(&mut self, at: Timestamp, station: &uniwrt_model::facts::StationFacts) {
        let now = Counters {
            at,
            packets: station.tx_packets,
            retries: station.tx_retries,
            failed: station.tx_failed,
        };
        let Some(before) = self.counters else {
            self.counters = Some(now);
            return;
        };
        if at <= before.at {
            return;
        }
        if now.packets < before.packets {
            // The counters started over: a new association.
            self.retries = None;
            self.counters = Some(now);
            return;
        }
        let sent = now.packets - before.packets;
        if sent < MIN_FRAMES {
            return;
        }
        #[expect(clippy::cast_precision_loss)]
        let percent = |now: Option<u64>, before: Option<u64>| {
            now.zip(before)
                .map(|(now, before)| now.saturating_sub(before) as f32 / sent as f32 * 100.0)
        };
        if let Some(retries) = percent(now.retries, before.retries) {
            let failed = percent(now.failed, before.failed).unwrap_or(0.0);
            self.retries = Some((retries, failed));
        }
        self.counters = Some(now);
    }

    /// What ties a randomized address to the device's others: a hostname that is not generic,
    /// else a DHCP client ID that is not just the address.
    pub(crate) fn group(&self) -> Option<String> {
        let mac = self.mac();
        if !mac.is_locally_administered() {
            return None;
        }
        if let Some(hostname) = &self.record.hostname {
            let name = hostname.to_ascii_lowercase();
            if name.len() >= 3 && !GENERIC.contains(&name.as_str()) {
                return Some(format!("name:{name}"));
            }
        }
        let id = self.client_id.as_deref()?.to_ascii_lowercase();
        let hex = mac.octets().iter().fold(String::new(), |mut hex, octet| {
            let _ = write!(hex, "{octet:02x}");
            hex
        });
        (!id.is_empty() && !id.ends_with(&hex)).then(|| format!("id:{id}"))
    }

    /// The view; `aliases` names each group's alias.
    pub(crate) fn view(&self, aliases: Option<&BTreeMap<String, String>>) -> Client {
        let group = self.group();
        let alias = self
            .record
            .alias
            .clone()
            .or_else(|| aliases?.get(group.as_ref()?).cloned());
        Client {
            mac: self.mac(),
            alias,
            hostname: self.record.hostname.clone(),
            vendor: self.vendor.map(str::to_owned),
            kind: self.kind,
            randomized: self.mac().is_locally_administered(),
            group,
            ips: self.ips.clone(),
            online: self.online,
            connection: self.connection.clone(),
            first_seen: self.record.first_seen,
            last_seen: self.record.last_seen,
            blocked: self.record.blocked,
            experience: self.experience.clone(),
            wifi: self.record.wifi.clone(),
        }
    }

    /// Where it was last, from a stored session, until it is seen again.
    pub(crate) fn restore_place(&mut self, session: &ClientSession) {
        if self.connection.is_some() {
            return;
        }
        self.connection = Some(Connection {
            node: session.node,
            upstream: session
                .node
                .map_or_else(String::new, |node| format!("node:{node}")),
            upstream_name: None,
            port: session.port.clone(),
            medium: session.medium,
            since: session.start,
            band: session.band,
            ssid: session.ssid.clone(),
            channel: None,
            signal_dbm: None,
            noise_dbm: None,
            rx_rate: None,
            tx_rate: None,
            standard: None,
            capabilities: Vec::new(),
            capacity_mbps: None,
            traffic: None,
        });
    }
}

fn placeholder() -> Client {
    Client {
        mac: MacAddr::BROADCAST,
        alias: None,
        hostname: None,
        vendor: None,
        kind: ClientKind::Unknown,
        randomized: false,
        group: None,
        ips: Vec::new(),
        online: false,
        connection: None,
        first_seen: Timestamp::UNIX_EPOCH,
        last_seen: Timestamp::UNIX_EPOCH,
        blocked: None,
        experience: None,
        wifi: ClientWifi::default(),
    }
}
