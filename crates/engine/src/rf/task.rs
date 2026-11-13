//! The scanner's task: the schedule, the scans and the report.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc, oneshot, watch,
};
use tracing::{debug, warn};
use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    events::{EventCategory, EventFilter, EventKind},
    facts::NodeFacts,
    rf::{NewScan, RadioScan, RfSettings, Scan, ScanState, ScanTrigger},
    status::NodeState,
};
use uniwrt_node::{
    Error as LinkError, NodeLink,
    facts::scan::heard,
    proto::ops::{self, Scanned, code},
};
use uniwrt_rf::{Inputs, RadarHit, RfView};
use uniwrt_store::{EventRepo, RfRepo, SettingsRepo};

use super::{
    Command, RfConfig, RfError, Snapshot,
    radios::{self, Key, Radio},
};
use crate::{Engine, Event};

/// The log pattern of hostapd's radar reports (`log-patterns.toml`).
const RADAR_PATTERN: &str = "dfs-radar";
/// The most radar events read at start.
const RADAR_READ: usize = 10_000;
/// How often old scans are deleted.
const PRUNE_EVERY: Duration = Duration::from_hours(1);
/// A scan refused because another runs on the phy is tried again this soon.
const RETRY_PHY_BUSY: Duration = Duration::from_mins(2);

/// A scan that ended.
struct Done {
    key: Key,
    trigger: ScanTrigger,
    at: Timestamp,
    result: Result<Scanned, LinkError>,
}

pub(super) struct Task<L, R> {
    engine: Engine<L>,
    repo: R,
    config: RfConfig,
    view: watch::Sender<Arc<RfView>>,
    settings: RfSettings,
    radios: BTreeMap<Key, Radio>,
    queue: VecDeque<Key>,
    running: Option<(Key, ScanTrigger)>,
    /// When the latest scan ended; the next waits `gap` after it.
    ended: Option<Timestamp>,
    /// Each radio's latest scan.
    scans: BTreeMap<Key, Scan>,
    radar: Vec<RadarHit>,
    /// How many radios were first seen, for the stagger.
    staggered: u32,
    dirty: bool,
    built: Option<Timestamp>,
    pruned: Option<Timestamp>,
    done: (mpsc::Sender<Done>, Option<mpsc::Receiver<Done>>),
}

impl<L, R> Task<L, R>
where
    L: NodeLink,
    R: RfRepo + EventRepo + SettingsRepo + Clone + 'static,
{
    pub(super) async fn new(
        engine: Engine<L>,
        repo: R,
        config: RfConfig,
        view: watch::Sender<Arc<RfView>>,
    ) -> Self {
        let now = engine.context().clock.now();
        let since = now.saturating_sub(config.keep);
        let settings = match repo.setting(RfSettings::KEY).await {
            Ok(Some(value)) => serde_json::from_value(value).unwrap_or_default(),
            Ok(None) => RfSettings::default(),
            Err(err) => {
                warn!(%err, "cannot read the RF settings: the defaults apply");
                RfSettings::default()
            }
        };
        let scans = match repo.latest_scans(since).await {
            Ok(scans) => scans
                .into_iter()
                .map(|s| ((s.node, s.radio.clone()), s))
                .collect(),
            Err(err) => {
                warn!(%err, "cannot read the latest scans");
                BTreeMap::new()
            }
        };
        let radar = radar_history(&repo, since).await;
        let mut task = Self {
            done: {
                let (sender, receiver) = mpsc::channel(4);
                (sender, Some(receiver))
            },
            engine,
            repo,
            config,
            view,
            settings,
            radios: BTreeMap::new(),
            queue: VecDeque::new(),
            running: None,
            ended: None,
            scans,
            radar,
            staggered: 0,
            dirty: true,
            built: None,
            pruned: None,
        };
        for (node, view) in task.engine.all() {
            if let Some(facts) = view.facts {
                task.facts(node, &facts);
            }
        }
        task.publish();
        task
    }

    pub(super) async fn run(
        mut self,
        mut bus: broadcast::Receiver<Event>,
        mut commands: mpsc::Receiver<Command>,
        mut stopped: oneshot::Receiver<()>,
    ) {
        let Some(mut done) = self.done.1.take() else {
            return;
        };
        let mut tick = tokio::time::interval(self.config.tick_every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                event = bus.recv() => match event {
                    Ok(event) => self.event(&event),
                    Err(RecvError::Lagged(missed)) => {
                        debug!(missed, "RF insight fell behind the bus");
                        self.dirty = true;
                    }
                    Err(RecvError::Closed) => break,
                },
                Some(command) = commands.recv() => self.command(command).await,
                Some(ended) = done.recv() => self.finished(ended).await,
                _ = tick.tick() => self.tick().await,
                _ = &mut stopped => break,
            }
            let now = self.now();
            if self.dirty
                && self
                    .built
                    .is_none_or(|at| now >= at.saturating_add(self.config.rebuild_after))
            {
                self.publish();
            }
        }
    }

    fn now(&self) -> Timestamp {
        self.engine.context().clock.now()
    }

    fn interval(&self) -> Duration {
        Duration::from_hours(u64::from(self.settings.interval_h.max(1)))
    }

    fn event(&mut self, event: &Event) {
        match event {
            Event::Facts { node, facts, .. } => self.facts(*node, facts),
            Event::Removed { node } => {
                self.radios.retain(|(n, _), _| n != node);
                self.scans.retain(|(n, _), _| n != node);
                self.queue.retain(|(n, _)| n != node);
                self.dirty = true;
            }
            Event::Status { .. } => self.dirty = true,
            Event::Logged { records } => {
                for record in records.iter() {
                    if let (Some(node), Some(RADAR_PATTERN)) =
                        (record.node, record.pattern.as_deref())
                        && let Some(span) = uniwrt_rf::radar::span(&record.entry.message)
                    {
                        self.radar.push(RadarHit {
                            node,
                            at: record.received,
                            span,
                        });
                        self.dirty = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn facts(&mut self, node: NodeId, facts: &NodeFacts) {
        let now = self.now();
        let (first, stagger) = (self.config.first_delay, self.config.stagger);
        let staggered = &mut self.staggered;
        radios::update(&mut self.radios, node, facts, || {
            let due = now.saturating_add(first + stagger * *staggered);
            *staggered += 1;
            due
        });
        // A radio scanned before (in an earlier run) is due an interval after that scan.
        let interval = self.interval();
        for (key, radio) in &mut self.radios {
            if radio.last.is_none()
                && let Some(scan) = self.scans.get(key)
            {
                radio.last = Some(scan.summary());
                radio.next_at = radio.next_at.max(scan.at.saturating_add(interval));
            }
        }
        self.dirty = true;
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::Scan { node, radio, done } => {
                let keys: Vec<Key> = self
                    .radios
                    .keys()
                    .filter(|(n, _)| node.is_none_or(|node| node == *n))
                    .filter(|(_, r)| radio.as_ref().is_none_or(|radio| radio == r))
                    .cloned()
                    .collect();
                if keys.is_empty() {
                    let _ = done.send(Err(RfError::NoRadio));
                    return;
                }
                for key in &keys {
                    let running = self.running.as_ref().is_some_and(|(k, _)| k == key);
                    if !running && !self.queue.contains(key) {
                        self.queue.push_back(key.clone());
                    }
                }
                self.publish();
                let queued = self
                    .radio_views()
                    .into_iter()
                    .filter(|r| {
                        keys.iter()
                            .any(|(n, name)| *n == r.node && *name == r.radio)
                    })
                    .collect();
                let _ = done.send(Ok(queued));
                self.tick().await;
            }
            Command::Settings { settings, done } => {
                let value = serde_json::to_value(settings).unwrap_or_default();
                if let Err(err) = self.repo.put_setting(RfSettings::KEY, value).await {
                    let _ = done.send(Err(RfError::Store(err.to_string())));
                    return;
                }
                let interval = Duration::from_hours(u64::from(settings.interval_h.max(1)));
                let now = self.now();
                for radio in self.radios.values_mut() {
                    if let (Some(last), ScanState::Scheduled) = (&radio.last, radio.state) {
                        radio.next_at = last.at.saturating_add(interval).max(now);
                    }
                }
                self.settings = settings;
                self.publish();
                let _ = done.send(Ok(()));
            }
            Command::Snapshot { done } => {
                if self.dirty {
                    self.publish();
                }
                let facts = self
                    .engine
                    .all()
                    .into_iter()
                    .filter_map(|(node, view)| Some((node, view.facts?)))
                    .collect();
                let _ = done.send(Snapshot {
                    facts,
                    scans: self.scans.values().cloned().collect(),
                    view: Arc::clone(&self.view.borrow()),
                });
            }
            Command::Busy { max_kbps, done } => {
                let busy = self
                    .radios
                    .iter()
                    .filter(|(_, radio)| !radio.idle(max_kbps))
                    .map(|(key, _)| key.clone())
                    .collect();
                let _ = done.send(busy);
            }
        }
    }

    async fn tick(&mut self) {
        let now = self.now();
        if self
            .pruned
            .is_none_or(|at| now >= at.saturating_add(PRUNE_EVERY))
        {
            self.pruned = Some(now);
            if let Err(err) = self
                .repo
                .prune_scans(now.saturating_sub(self.config.keep))
                .await
            {
                warn!(%err, "cannot delete old scans");
            }
            let since = now.saturating_sub(self.config.keep);
            self.radar.retain(|hit| hit.at >= since);
        }
        if self.running.is_some()
            || self
                .ended
                .is_some_and(|at| now < at.saturating_add(self.config.gap))
        {
            return;
        }
        while let Some(key) = self.queue.pop_front() {
            if self.ready(&key) {
                self.start(key, ScanTrigger::Manual);
                return;
            }
            if let Some(radio) = self.radios.get_mut(&key) {
                radio.error = Some("the radio cannot be scanned now".to_owned());
                self.dirty = true;
            }
        }
        if !self.settings.scanning {
            return;
        }
        let mut due: Vec<Key> = self
            .radios
            .iter()
            .filter(|(_, r)| r.next_at <= now)
            .map(|(k, _)| k.clone())
            .collect();
        due.sort_by_key(|k| self.radios[k].next_at);
        for key in due {
            if !self.ready(&key) {
                continue;
            }
            let (idle_kbps, retry) = (self.config.idle_kbps, self.config.retry_busy);
            let Some(radio) = self.radios.get_mut(&key) else {
                continue;
            };
            if radio.idle(idle_kbps) {
                self.start(key, ScanTrigger::Scheduled);
                return;
            }
            radio.state = ScanState::Busy;
            radio.next_at = now.saturating_add(retry);
            self.dirty = true;
        }
    }

    /// The radio can be scanned: up, with an interface, on a node online.
    fn ready(&self, key: &Key) -> bool {
        let online = self.engine.node(key.0).is_some_and(|view| {
            matches!(view.status.state, NodeState::Online | NodeState::Degraded)
        });
        online && self.radios.get(key).is_some_and(Radio::scannable)
    }

    fn start(&mut self, key: Key, trigger: ScanTrigger) {
        let (Some(link), Some(radio)) = (self.engine.link(key.0), self.radios.get(&key)) else {
            return;
        };
        let Some(ifname) = radio.ifname.clone() else {
            return;
        };
        let op = ops::Scan::new(ifname, radio.freqs.clone());
        let (timeout, done, at) = (self.config.timeout, self.done.0.clone(), self.now());
        let scanned = key.clone();
        tokio::spawn(async move {
            let result = link.request(&op, timeout).await;
            let _ = done
                .send(Done {
                    key: scanned,
                    trigger,
                    at,
                    result,
                })
                .await;
        });
        self.running = Some((key, trigger));
        self.publish();
    }

    async fn finished(&mut self, done: Done) {
        let now = self.now();
        self.running = None;
        self.ended = Some(now);
        let interval = self.interval();
        let Some(band) = self.radios.get(&done.key).map(|r| r.band) else {
            return;
        };
        let (next_at, state, error) = match done.result {
            Ok(scanned) => {
                let scan = NewScan {
                    node: done.key.0,
                    radio: done.key.1.clone(),
                    band,
                    at: done.at,
                    trigger: done.trigger,
                    heard: heard(&scanned, band),
                };
                match self.repo.add_scan(scan).await {
                    Ok(scan) => {
                        if let Some(radio) = self.radios.get_mut(&done.key) {
                            radio.last = Some(scan.summary());
                        }
                        self.scans.insert(done.key.clone(), scan);
                        (done.at.saturating_add(interval), ScanState::Scheduled, None)
                    }
                    Err(err) => {
                        warn!(%err, "cannot store a scan");
                        let error = format!("the scan cannot be stored: {err}");
                        (
                            now.saturating_add(self.config.retry_failed),
                            ScanState::Scheduled,
                            Some(error),
                        )
                    }
                }
            }
            Err(LinkError::Remote { code, msg, .. }) if code == code::UNSUPPORTED => (
                now.saturating_add(self.config.retry_unsupported),
                ScanState::Unsupported,
                Some(msg),
            ),
            Err(LinkError::Remote { msg, .. }) if msg.starts_with("busy") => (
                now.saturating_add(RETRY_PHY_BUSY),
                ScanState::Scheduled,
                Some(msg),
            ),
            Err(err) => (
                now.saturating_add(self.config.retry_failed),
                ScanState::Scheduled,
                Some(err.to_string()),
            ),
        };
        if let Some(radio) = self.radios.get_mut(&done.key) {
            radio.next_at = next_at;
            radio.state = state;
            radio.error = error;
        }
        self.publish();
    }

    /// Builds the view again and announces it if it changed.
    fn publish(&mut self) {
        let now = self.now();
        self.built = Some(now);
        self.dirty = false;
        let views: Vec<_> = self.engine.all();
        let facts: Vec<(NodeId, Arc<NodeFacts>)> = views
            .into_iter()
            .filter_map(|(node, view)| Some((node, view.facts?)))
            .collect();
        let nodes: Vec<(NodeId, &NodeFacts)> = facts.iter().map(|(n, f)| (*n, &**f)).collect();
        let scans: Vec<Scan> = self.scans.values().cloned().collect();
        let inputs = Inputs {
            nodes: &nodes,
            scans: &scans,
            radar: &self.radar,
        };
        let report = uniwrt_rf::report(&inputs, |mac| {
            uniwrt_clients::vendor(mac).map(str::to_owned)
        });
        let view = RfView {
            report,
            radios: self.radio_views(),
            settings: self.settings,
        };
        if **self.view.borrow() == view {
            return;
        }
        let view = Arc::new(view);
        self.view.send_replace(Arc::clone(&view));
        self.engine.context().publish(Event::Rf { view });
    }

    fn radio_views(&self) -> Vec<RadioScan> {
        self.radios
            .iter()
            .map(|(key, radio)| {
                let state = if self.running.as_ref().is_some_and(|(k, _)| k == key) {
                    ScanState::Scanning
                } else if self.queue.contains(key) {
                    ScanState::Queued
                } else if !self.ready(key) {
                    ScanState::Unavailable
                } else if radio.state == ScanState::Unsupported {
                    ScanState::Unsupported
                } else if !self.settings.scanning {
                    ScanState::Off
                } else {
                    radio.state
                };
                radio.view(key, state)
            })
            .collect()
    }
}

/// The radar hits of the events since `since` (each `dfs-radar` log event, once however often
/// it folded).
async fn radar_history<R: EventRepo>(repo: &R, since: Timestamp) -> Vec<RadarHit> {
    let filter = EventFilter {
        categories: vec![EventCategory::Wifi],
        from: Some(since),
        ..EventFilter::default()
    };
    match repo.events(filter, None, RADAR_READ).await {
        Ok(events) => events
            .into_iter()
            .filter_map(|event| {
                let EventKind::LogPattern { pattern, text, .. } = &event.what else {
                    return None;
                };
                let span = (pattern == RADAR_PATTERN)
                    .then(|| uniwrt_rf::radar::span(text))
                    .flatten()?;
                Some(RadarHit {
                    node: event.node?,
                    at: event.at,
                    span,
                })
            })
            .collect(),
        Err(err) => {
            warn!(%err, "cannot read the radar history");
            Vec::new()
        }
    }
}
