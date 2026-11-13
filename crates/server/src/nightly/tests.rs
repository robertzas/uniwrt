//! The nightly optimizer's decisions over a scripted site: a node meshing on 5 GHz, whose plan
//! improves the band by 30 %, at 03:40 local in the window opening at 03:30; its radio1 catches
//! up after a channel switch where the test says so.

// The fake answers at once.
#![allow(clippy::unused_async_trait_impl)]

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};

use uniwrt_core::{JobId, NodeId, RevisionId, Timestamp};
use uniwrt_model::{
    events::EventKind,
    node::NodeMode,
    rf::{BandGain, NightlySkip, RfSettings},
    site::SiteConfig,
    wifi::{Band, ChannelWidth},
};
use uniwrt_rf::plan::{BandPlan, Breakdown, Plan, RadioPlan, Search, Setting};

use super::{Head, SaveError, Site, StartError, State, look};

const NODE: NodeId = NodeId::new(1);
/// Day 20405, 03:40 UTC.
const NIGHT: i64 = 20_405;

fn at(h: i64, m: i64) -> Timestamp {
    Timestamp::from_unix_ms((NIGHT * 86_400 + h * 3600 + m * 60) * 1000)
}

fn plan(improvement: f32) -> Plan {
    let setting = |channel: u16| Setting {
        channel,
        width: ChannelWidth::Mhz80,
        center_frequency_mhz: 5000 + 5 * u32::from(channel) + 30,
        tx_power_dbm: None,
    };
    Plan {
        bands: vec![BandPlan {
            band: Band::G5,
            radios: vec![RadioPlan {
                node: NODE,
                radio: "radio1".to_owned(),
                current: Some(setting(36)),
                planned: setting(149),
                changed: true,
                before: Some(Breakdown::default()),
                after: Breakdown::default(),
                reasons: Vec::new(),
            }],
            before: 1.0,
            after: 1.0 - improvement,
            improvement,
            search: Search::Exhaustive,
            optimal: true,
            skipped: Vec::new(),
        }],
    }
}

// The fake's switches, one per thing a test turns on.
#[allow(clippy::struct_excessive_bools)]
struct World {
    settings: RfSettings,
    now: Timestamp,
    head: RevisionId,
    config: SiteConfig,
    holds: Option<RevisionId>,
    plan: Plan,
    busy: bool,
    offline: bool,
    apply_busy: bool,
    /// Set by an apply that starts, as the real job holds the configuration until it ends.
    job_running: bool,
    catching_up: Vec<String>,
    restart_busy: bool,
    restarts: Vec<Vec<NodeId>>,
    saved: Vec<(RevisionId, String)>,
    applied: Vec<RevisionId>,
    notes: Vec<EventKind>,
}

struct Fake(Mutex<World>);

impl Fake {
    fn new() -> Self {
        let mut config = SiteConfig::defaults(None);
        config.site.timezone = "UTC".to_owned();
        Self(Mutex::new(World {
            settings: RfSettings::default(),
            now: at(3, 40),
            head: RevisionId::new(7),
            config,
            holds: Some(RevisionId::new(7)),
            plan: plan(0.3),
            busy: false,
            offline: false,
            apply_busy: false,
            job_running: false,
            catching_up: Vec::new(),
            restart_busy: false,
            restarts: Vec::new(),
            saved: Vec::new(),
            applied: Vec::new(),
            notes: Vec::new(),
        }))
    }

    fn with<T>(&self, f: impl FnOnce(&mut World) -> T) -> T {
        f(&mut self.0.lock().unwrap())
    }
}

impl Site for Fake {
    fn settings(&self) -> RfSettings {
        self.with(|w| w.settings)
    }

    fn now(&self) -> Timestamp {
        self.with(|w| w.now)
    }

    async fn head(&self) -> Result<Option<Head>, String> {
        Ok(Some(self.with(|w| Head {
            id: w.head,
            config: w.config.clone(),
        })))
    }

    async fn nodes(
        &self,
    ) -> Result<(BTreeMap<NodeId, NodeMode>, BTreeMap<NodeId, RevisionId>), String> {
        Ok(self.with(|w| {
            (
                BTreeMap::from([(NODE, NodeMode::Managed)]),
                w.holds.map(|r| (NODE, r)).into_iter().collect(),
            )
        }))
    }

    async fn plan(&self, _: SiteConfig, _: BTreeMap<NodeId, NodeMode>) -> Result<Plan, String> {
        Ok(self.with(|w| w.plan.clone()))
    }

    async fn busy(&self) -> Result<BTreeSet<(NodeId, String)>, String> {
        Ok(self.with(|w| {
            w.busy
                .then(|| (NODE, "radio1".to_owned()))
                .into_iter()
                .collect()
        }))
    }

    fn catching_up(&self, _: NodeId) -> Vec<String> {
        self.with(|w| w.catching_up.clone())
    }

    fn offline(&self, _: NodeId) -> bool {
        self.with(|w| w.offline)
    }

    async fn job_running(&self) -> bool {
        self.with(|w| w.job_running)
    }

    async fn save(
        &self,
        base: RevisionId,
        config: SiteConfig,
        message: String,
    ) -> Result<RevisionId, SaveError> {
        self.with(|w| {
            if base != w.head {
                return Err(SaveError::NotLatest);
            }
            w.head = RevisionId::new(w.head.get() + 1);
            w.config = config;
            w.saved.push((w.head, message));
            Ok(w.head)
        })
    }

    async fn apply(&self, revision: RevisionId) -> Result<JobId, StartError> {
        self.with(|w| {
            if w.apply_busy {
                return Err(StartError::Busy);
            }
            w.applied.push(revision);
            w.job_running = true;
            Ok(JobId::new(40 + i64::try_from(w.applied.len()).unwrap_or(0)))
        })
    }

    async fn restart(&self, nodes: Vec<NodeId>) -> Result<JobId, StartError> {
        self.with(|w| {
            if w.restart_busy {
                return Err(StartError::Busy);
            }
            w.restarts.push(nodes);
            Ok(JobId::new(60))
        })
    }

    fn note(&self, kind: EventKind) {
        self.with(|w| w.notes.push(kind));
    }
}

fn gains() -> Vec<BandGain> {
    vec![BandGain {
        band: Band::G5,
        percent: 30,
    }]
}

fn skipped(reason: NightlySkip) -> EventKind {
    EventKind::RfOptimizeSkipped {
        reason,
        bands: gains(),
    }
}

#[tokio::test]
async fn the_window_saves_the_plan_by_the_controller_and_applies_it_once_a_night() {
    let site = Fake::new();
    let mut state = State::default();
    site.with(|w| w.now = at(3, 20));
    look(&site, &mut state).await;
    assert!(site.with(|w| w.saved.is_empty()), "not before 03:30");

    site.with(|w| w.now = at(3, 40));
    look(&site, &mut state).await;
    let revision = RevisionId::new(8);
    site.with(|w| {
        assert_eq!(
            w.saved,
            vec![(revision, "Nightly RF plan: 5 GHz 30 % better".to_owned())]
        );
        let planned = &w.config.radio.plan.radios;
        assert_eq!((planned.len(), planned[0].channel), (1, 149));
        assert_eq!(w.applied, vec![revision]);
        assert_eq!(
            w.notes,
            vec![EventKind::RfOptimized {
                revision,
                job: JobId::new(41),
                bands: gains(),
            }]
        );
    });

    // Later that night nothing more happens, even if the radios didn't move.
    site.with(|w| w.now = at(3, 50));
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| w.applied.len()), 1);

    // The apply failed (the node still holds 7): the next night doesn't try it again by itself.
    site.with(|w| w.now = at(27, 40));
    look(&site, &mut state).await;
    site.with(|w| {
        assert_eq!((w.saved.len(), w.applied.len()), (1, 1));
        assert_eq!(w.notes.last(), Some(&skipped(NightlySkip::PendingChanges)));
    });
}

#[tokio::test]
async fn too_little_gain_or_the_optimizer_off_moves_nothing() {
    let site = Fake::new();
    site.with(|w| w.plan = plan(0.1));
    look(&site, &mut State::default()).await;
    site.with(|w| w.plan = plan(0.3));
    site.with(|w| w.settings.nightly = false);
    look(&site, &mut State::default()).await;
    site.with(|w| assert!(w.saved.is_empty() && w.applied.is_empty() && w.notes.is_empty()));
}

#[tokio::test]
async fn unapplied_changes_skip_the_night() {
    let site = Fake::new();
    let mut state = State::default();
    site.with(|w| w.holds = Some(RevisionId::new(6)));
    look(&site, &mut state).await;
    site.with(|w| w.now = at(4, 0));
    look(&site, &mut state).await;
    site.with(|w| {
        assert!(w.saved.is_empty() && w.applied.is_empty());
        assert_eq!(w.notes, vec![skipped(NightlySkip::PendingChanges)]);
    });
}

#[tokio::test]
async fn heavy_traffic_or_an_offline_node_waits_and_a_window_closing_on_it_is_noted() {
    let site = Fake::new();
    let mut state = State::default();
    site.with(|w| w.busy = true);
    look(&site, &mut state).await;
    assert!(site.with(|w| w.saved.is_empty() && w.notes.is_empty()));
    // Quiet again within the window, but the node dropped off: still waiting.
    site.with(|w| (w.busy, w.offline, w.now) = (false, true, at(4, 0)));
    look(&site, &mut state).await;
    assert!(site.with(|w| w.saved.is_empty() && w.notes.is_empty()));
    site.with(|w| w.now = at(5, 40));
    look(&site, &mut state).await;
    site.with(|w| assert_eq!(w.notes, vec![skipped(NightlySkip::Offline)]));

    // The next night it is quiet.
    site.with(|w| (w.offline, w.now) = (false, at(27, 30)));
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| w.applied.len()), 1);
}

#[tokio::test]
async fn an_apply_that_cannot_start_starts_later_from_the_revision_already_saved() {
    let site = Fake::new();
    let mut state = State::default();
    site.with(|w| w.apply_busy = true);
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| (w.saved.len(), w.applied.len())), (1, 0));
    // The node doesn't hold the optimizer's own revision, which isn't a change waiting for review.
    site.with(|w| (w.apply_busy, w.now) = (false, at(3, 50)));
    look(&site, &mut state).await;
    site.with(|w| {
        assert_eq!(w.saved.len(), 1, "not saved again");
        assert_eq!(w.applied, vec![RevisionId::new(8)]);
    });
}

#[tokio::test]
async fn radios_catching_up_restart_after_tonights_apply_once_a_night() {
    let site = Fake::new();
    let mut state = State::default();
    site.with(|w| w.catching_up = vec!["radio1".to_owned()]);
    look(&site, &mut state).await;
    site.with(|w| {
        assert_eq!(w.applied.len(), 1);
        assert!(w.restarts.is_empty(), "not while tonight's apply runs");
    });
    site.with(|w| (w.job_running, w.now) = (false, at(3, 50)));
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| w.restarts.clone()), vec![vec![NODE]]);
    // Still catching up later that night (the restart failed): not restarted again.
    site.with(|w| w.now = at(4, 0));
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| w.restarts.len()), 1);
    // The next night it is, even though the optimizer skips (its revision unapplied).
    site.with(|w| w.now = at(27, 40));
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| w.restarts.len()), 2);
}

#[tokio::test]
async fn with_the_optimizer_off_the_window_still_restarts_radios_once_quiet() {
    let site = Fake::new();
    let mut state = State::default();
    site.with(|w| {
        w.settings.nightly = false;
        w.catching_up = vec!["radio1".to_owned()];
        w.busy = true;
        w.now = at(3, 20);
    });
    look(&site, &mut state).await;
    assert!(site.with(|w| w.restarts.is_empty()), "not before 03:30");
    site.with(|w| w.now = at(3, 40));
    look(&site, &mut state).await;
    assert!(
        site.with(|w| w.restarts.is_empty()),
        "not with heavy traffic"
    );
    // Quiet, but another job took the node; then the next look starts it.
    site.with(|w| (w.busy, w.restart_busy, w.now) = (false, true, at(3, 50)));
    look(&site, &mut state).await;
    assert!(site.with(|w| w.restarts.is_empty()));
    site.with(|w| (w.restart_busy, w.now) = (false, at(4, 0)));
    look(&site, &mut state).await;
    site.with(|w| {
        assert_eq!(w.restarts, vec![vec![NODE]]);
        assert!(w.saved.is_empty() && w.applied.is_empty() && w.notes.is_empty());
    });
    // An offline node is left alone.
    site.with(|w| (w.offline, w.now) = (true, at(28, 0)));
    look(&site, &mut state).await;
    assert_eq!(site.with(|w| w.restarts.len()), 1);
}
