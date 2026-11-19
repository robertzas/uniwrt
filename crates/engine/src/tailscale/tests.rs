//! The Tailscale task over a store, a vault and an engine watching a scripted gateway whose
//! `services` set reports Tailscale as given, with the `tailscale` op answered by hand: prefs
//! that differ get `set`, a logged-out daemon with a key in the vault logs in with it, a stopped
//! one comes up and an unwanted one goes down; an observed gateway is left alone; a refusal
//! raises a condition that clears once the gateway takes a request. And [`step`] alone.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::sync::broadcast;
use uniwrt_core::{MacAddr, ManualClock, NodeId, Timestamp};
use uniwrt_model::{
    events::EventKind,
    facts::{TailscaleFacts, TailscalePrefs, TailscaleState},
    node::NodeRole,
    site::{
        SecretRef, SiteConfig,
        node::{ManagedPackage, NodeConfig},
        revision::{NewRevision, RevisionKind},
        vpn::tailscale::Tailscale,
    },
};
use uniwrt_node::{
    Error,
    proto::ops::{Set, TailscaleAction},
};
use uniwrt_store::{MasterKey, NodeRepo as _, RevisionRepo as _, Store, Vault};

use super::{TailscaleConfig, TailscaleSync, step};
use crate::{
    Engine, EngineConfig, Event, Identity,
    testing::{self, Call, Script, hello, running},
};

const START: Timestamp = Timestamp::from_unix_secs(1_800_000_000);
const KEY: &str = "tskey-auth-kTEST-0123456789";

fn held() -> TailscalePrefs {
    TailscalePrefs {
        want_running: true,
        logged_out: false,
        advertise_routes: Vec::new(),
        hostname: None,
        accept_dns: false,
        accept_routes: false,
        netfilter_off: true,
    }
}

fn facts(state: TailscaleState, prefs: TailscalePrefs) -> TailscaleFacts {
    TailscaleFacts {
        state: Some(state),
        prefs: Some(prefs),
        ..TailscaleFacts::default()
    }
}

/// The site's Tailscale: the default LAN advertised, the key's reference where given.
fn site(node: NodeId, managed: bool, key: Option<SecretRef>) -> SiteConfig {
    let mut config = SiteConfig::defaults(None);
    let lan = config.networks[0].id.clone();
    let mut tailscale = Tailscale::new(vec![lan]);
    tailscale.auth_key = key;
    config.vpn.tailscale = Some(tailscale);
    let mut entry = NodeConfig::new(node);
    if managed {
        entry.managed = BTreeSet::from([ManagedPackage::System]);
    }
    config.nodes = vec![entry];
    config
}

#[test]
fn only_what_differs_from_the_site_is_wanted() {
    let config = site(NodeId::new(1), true, None);
    let lan = config.networks[0]
        .ipv4
        .as_ref()
        .unwrap()
        .address
        .trunc()
        .to_string();
    let mut prefs = held();
    prefs.advertise_routes = vec![lan];
    let action = |config: &SiteConfig, facts: &TailscaleFacts| step(config, facts).map(|(a, _)| a);
    // Held, running: nothing. Prefs off: `set`, whatever else.
    let ok = facts(TailscaleState::Running, prefs.clone());
    assert_eq!(action(&config, &ok), None);
    let stale = facts(TailscaleState::NeedsLogin, held());
    assert_eq!(action(&config, &stale), Some(TailscaleAction::Set));
    let (_, sent) = step(&config, &stale).unwrap();
    assert_eq!(sent.routes, prefs.advertise_routes);
    // Logged out: a login only with a key.
    let out = facts(TailscaleState::NeedsLogin, prefs.clone());
    assert_eq!(action(&config, &out), None);
    let keyed = site(NodeId::new(1), true, Some(SecretRef::numbered(1)));
    assert_eq!(action(&keyed, &out), Some(TailscaleAction::Login));
    // Stopped after `down`: `up`; after a logout it needs a login instead.
    let mut down = prefs.clone();
    down.want_running = false;
    assert_eq!(
        action(&config, &facts(TailscaleState::Stopped, down.clone())),
        Some(TailscaleAction::Up)
    );
    down.logged_out = true;
    assert_eq!(action(&config, &facts(TailscaleState::Stopped, down)), None);
    // Off in the site: `down` while it runs, then nothing; the prefs don't matter.
    let mut off = config.clone();
    off.vpn.tailscale.as_mut().unwrap().enabled = false;
    assert_eq!(
        action(&off, &facts(TailscaleState::Running, held())),
        Some(TailscaleAction::Down)
    );
    let mut stopped = held();
    stopped.want_running = false;
    assert_eq!(action(&off, &facts(TailscaleState::Stopped, stopped)), None);
    // Never set up, or a daemon that doesn't answer: left alone.
    let mut none = config.clone();
    none.vpn.tailscale = None;
    assert_eq!(action(&none, &stale), None);
    assert_eq!(action(&config, &TailscaleFacts::default()), None);
}

struct Harness {
    _dir: tempfile::TempDir,
    engine: Engine<testing::SharedLink>,
    bus: broadcast::Receiver<Event>,
    sync: Option<TailscaleSync>,
    script: Script,
    refuse: Arc<Mutex<Option<String>>>,
}

impl Harness {
    /// A gateway reporting `reported` (the `services` set's `tailscale`), in a site whose
    /// Tailscale has the auth key `KEY` in the vault where `keyed`.
    async fn start(reported: Value, managed: bool, keyed: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(START));
        let store = Store::open(dir.path().join("test.db"), clock.clone())
            .await
            .unwrap();
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let mac = MacAddr::new([2, 0, 0, 0, 0, 1]);
        let node = store
            .create_node(uniwrt_model::node::NewNode {
                name: "gw".to_owned(),
                mac,
                board_name: "x".to_owned(),
                host: "10.0.0.1".to_owned(),
                port: 22,
                role: NodeRole::Gateway,
            })
            .await
            .unwrap()
            .id;
        let sealed: Vec<_> = keyed
            .then(|| vault.seal_config_secret(KEY.as_bytes()).unwrap())
            .into_iter()
            .collect();
        let key = sealed.first().map(|s| s.reference().clone());
        store
            .commit_revision(
                NewRevision {
                    base: None,
                    kind: RevisionKind::Defaults,
                    restored_from: None,
                    author_id: None,
                    author: None,
                    message: "test".to_owned(),
                    config: site(node, managed, key),
                },
                sealed,
            )
            .await
            .unwrap();
        let engine = Engine::new(EngineConfig::default(), clock.clone());
        let bus = engine.subscribe();
        let config = TailscaleConfig {
            tick_every: Duration::from_millis(20),
            ..TailscaleConfig::default()
        };
        let sync = TailscaleSync::start(&engine, store, Some(vault), None, config);
        let (link, script) = testing::link();
        let refuse = Arc::new(Mutex::new(None::<String>));
        let refusal = Arc::clone(&refuse);
        script.answer(move |op, args| match op {
            "collect" => {
                let sets: Vec<Set> = serde_json::from_value(args["sets"].clone()).unwrap();
                let mut answer = testing::collected(&sets);
                if let Some(services) = answer["sets"].get_mut("services") {
                    services["tailscale"] = reported.clone();
                }
                Some(Ok(answer))
            }
            "tailscale" => Some(match refusal.lock().unwrap().clone() {
                Some(msg) => Err(Error::Remote {
                    op: "tailscale".to_owned(),
                    code: "failed".to_owned(),
                    msg,
                }),
                None => Ok(json!({"state": "Running", "auth_url": null})),
            }),
            _ => None,
        });
        let identity = Identity {
            role: NodeRole::Gateway,
            mac,
        };
        engine.add(node, identity, link, None).await;
        script.set_state(running(hello(true)));
        Self {
            _dir: dir,
            engine,
            bus,
            sync: Some(sync),
            script,
            refuse,
        }
    }

    /// The next `tailscale` request within `wait`, if one comes.
    async fn request(&mut self, wait: Duration) -> Option<Value> {
        let call: Call = tokio::time::timeout(wait, self.script.call_of("tailscale"))
            .await
            .ok()?;
        Some(call.args)
    }

    async fn noted(&mut self) -> uniwrt_model::events::NewEvent {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), self.bus.recv()).await {
                Ok(Ok(Event::Noted { event })) => return event,
                Ok(Ok(_) | Err(broadcast::error::RecvError::Lagged(_))) => {}
                other => panic!("no event noted: {other:?}"),
            }
        }
    }

    async fn stop(mut self) {
        if let Some(sync) = self.sync.take() {
            sync.stop().await;
        }
        self.engine.shutdown().await;
    }
}

/// What a fresh install reports: logged out, tailscaled's default prefs.
fn fresh() -> Value {
    json!({"state": "NeedsLogin", "prefs": {
        "want_running": false, "logged_out": true, "routes": null, "hostname": "",
        "accept_dns": true, "accept_routes": true, "netfilter": 2
    }})
}

/// Logged out with the site's prefs.
fn logged_out() -> Value {
    json!({"state": "NeedsLogin", "prefs": {
        "want_running": false, "logged_out": true, "routes": ["10.0.0.0/24"], "hostname": "",
        "accept_dns": false, "accept_routes": false, "netfilter": 0
    }})
}

#[tokio::test]
async fn a_fresh_daemon_gets_the_site_s_prefs() {
    let mut h = Harness::start(fresh(), true, false).await;
    let args = h.request(Duration::from_secs(10)).await.expect("a request");
    assert_eq!(args["action"], "set");
    assert_eq!(args["prefs"]["exit_node"], false);
    assert_eq!(args["prefs"]["hostname"], "");
    assert!(args.get("auth_key").is_none());
    // The same request waits `retry` (stale facts).
    assert!(h.request(Duration::from_millis(300)).await.is_none());
    h.stop().await;
}

#[tokio::test]
async fn a_logged_out_daemon_logs_in_with_the_vault_s_key() {
    let mut h = Harness::start(logged_out(), true, true).await;
    let args = h.request(Duration::from_secs(10)).await.expect("a login");
    // The defaults' LAN, as the site's prefs: held already, so a login comes straight away.
    assert_eq!(args["action"], "login");
    assert_eq!(args["auth_key"], KEY);
    h.stop().await;
}

#[tokio::test]
async fn without_a_key_a_login_waits_for_an_admin() {
    let mut h = Harness::start(logged_out(), true, false).await;
    assert!(h.request(Duration::from_millis(500)).await.is_none());
    h.stop().await;
}

#[tokio::test]
async fn an_observed_gateway_is_left_alone() {
    let mut h = Harness::start(fresh(), false, true).await;
    assert!(h.request(Duration::from_millis(500)).await.is_none());
    h.stop().await;
}

#[tokio::test]
async fn a_refusal_raises_a_condition() {
    let mut h = Harness::start(logged_out(), true, true).await;
    *h.refuse.lock().unwrap() = Some("backend error: invalid key".to_owned());
    h.request(Duration::from_secs(10)).await.expect("a login");
    let event = h.noted().await;
    assert_eq!(
        event.what,
        EventKind::TailscaleFailed {
            error: "backend error: invalid key".to_owned()
        }
    );
    assert_eq!(event.condition.as_ref().map(|c| c.raised), Some(true));
    h.stop().await;
}
