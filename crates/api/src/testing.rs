//! Test support: an API over a temporary store (its jobs reach nodes over SSH, or nothing
//! without a dial), and [`Demo`], the simulated fleet adopted into one with an admin logged in,
//! served over the router or a loopback socket.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{self, Body},
    extract::Request,
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::Response,
};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uniwrt_core::{Clock, NodeId, SystemClock};
use uniwrt_engine::{
    AlertEngine, Alerts, AlertsConfig, DriftConfig, DriftWatch, Drifts, Engine, EngineConfig,
    Fleet, LogRecorder, LogsConfig, Notifications, Notify, NotifyConfig, Rf, RfConfig, RfScanner,
};
use uniwrt_jobs::{BoxFuture, Dial, Jobs, JobsConfig, SshDial, Watch};
use uniwrt_model::{
    facts::NodeFacts,
    node::{NewNode, Node},
};
use uniwrt_node::{ControllerKey, HostKey, SshConfig};
use uniwrt_notify::Sender;
use uniwrt_sim::{SimConfig, SimFleet, SimLink};
use uniwrt_store::{MasterKey, NodeRepo as _, Store, Vault};

use crate::{Api, ApiConfig, BuildInfo, Certificate, Client, PasswordCost, TlsInfo, router};

/// The host the test clients address, and the origin of their pages.
pub(crate) const HOST: &str = "uniwrt.lan:8443";
pub(crate) const ORIGIN: &str = "https://uniwrt.lan:8443";
pub(crate) const PASSWORD: &str = "correct horse battery";

pub(crate) struct FakeCertificate;

impl Certificate for FakeCertificate {
    fn info(&self) -> TlsInfo {
        TlsInfo {
            ca_name: "uniwrt local CA test".into(),
            ca_sha256: "AA".into(),
            ca_not_after: "2036-10-29T00:00:00Z".into(),
            names: vec!["uniwrt.lan".into()],
            not_after: "2027-12-01T00:00:00Z".into(),
        }
    }

    fn ca_pem(&self) -> String {
        "-----BEGIN CERTIFICATE-----\n".into()
    }
}

/// An API at the cheapest password cost, whose jobs reach nodes over SSH.
pub(crate) async fn api(store: Store, fleet: Arc<dyn Fleet>, clock: Arc<dyn Clock>) -> Api {
    let dial = SshDial {
        config: SshConfig {
            connect_timeout: Duration::from_secs(2),
            ..SshConfig::default()
        },
    };
    api_with(store, fleet, clock, Some(Arc::new(dial))).await
}

/// Logs `admin` in; returns the session cookie, `name=value`.
pub(crate) async fn login(api: &Api) -> String {
    let login = json!({ "username": "admin", "password": PASSWORD });
    let res = router_send(api, post("/v1/auth/login", &login, None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let set = res.headers()[header::SET_COOKIE].to_str().unwrap();
    set.split(';').next().unwrap().to_owned()
}

/// Sends a request through `api`'s router, as the HTTPS listener hands it over.
pub(crate) async fn router_send(api: &Api, request: Request) -> Response {
    router(api.clone())
        .oneshot(with_client(request))
        .await
        .unwrap()
}

/// Starts watching nobody: the API's tests adopt nothing for real.
struct NoWatch;

impl Watch for NoWatch {
    fn watch(&self, _: Node, _: HostKey, _: NodeFacts) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }

    fn moved(&self, _: Node, _: HostKey) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// [`api`] whose jobs reach nodes with `dial`; `None` reaches nothing.
pub(crate) async fn api_with(
    store: Store,
    fleet: Arc<dyn Fleet>,
    clock: Arc<dyn Clock>,
    dial: Option<Arc<dyn Dial>>,
) -> Api {
    api_full(store, fleet, clock, dial, None, None, None, None).await
}

/// [`api_with`], with the alert engine's, the notifications', RF insight's and drift
/// detection's handles.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn api_full(
    store: Store,
    fleet: Arc<dyn Fleet>,
    clock: Arc<dyn Clock>,
    dial: Option<Arc<dyn Dial>>,
    alerts: Option<Alerts>,
    notify: Option<(Notify, Vault)>,
    rf: Option<Rf>,
    drift: Option<Drifts>,
) -> Api {
    let vault = match &notify {
        Some((_, vault)) => vault.clone(),
        None => Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap(),
    };
    let jobs = Jobs::start(JobsConfig {
        vault: vault.clone(),
        store: store.clone(),
        clock: Arc::clone(&clock),
        key: ControllerKey::generate().unwrap(),
        dial,
        watch: Arc::new(NoWatch),
    })
    .await
    .unwrap();
    Api::new(ApiConfig {
        password_cost: PasswordCost::CHEAPEST,
        alerts,
        notify,
        rf,
        drift,
        vault: Some(vault),
        ..ApiConfig::new(
            store,
            fleet,
            clock,
            BuildInfo {
                version: "1.2.3",
                commit: "abc",
                built_at: "2026-10-30T00:00:00Z",
            },
            Arc::new(FakeCertificate),
            jobs,
        )
    })
    .await
    .unwrap()
}

/// The simulated fleet (no reboots), adopted, watched, and an admin logged in.
pub(crate) struct Demo {
    _dir: tempfile::TempDir,
    pub(crate) store: Store,
    pub(crate) engine: Engine<SimLink>,
    _sim: SimFleet,
    pub(crate) api: Api,
    /// The admin's session cookie, `name=value`.
    pub(crate) cookie: String,
    /// The nodes' IDs: the gateway first.
    pub(crate) ids: Vec<NodeId>,
    /// Stores the nodes' logs, once started.
    logs: Option<LogRecorder>,
    /// Evaluates the alert rules (the API's alert routes go through it).
    _alerts: AlertEngine,
    _notifications: Notifications,
    /// Scans the simulated radios at once, one after another, busy or not.
    _rf: RfScanner,
    /// Watches for drift, reading what the store confirmed every 100 ms.
    _drift: DriftWatch,
}

impl Demo {
    pub(crate) async fn start() -> Self {
        Self::start_with(true).await
    }

    /// Without `dial`: nothing is adopted or probed.
    pub(crate) async fn start_with(dial: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let store = Store::open(dir.path().join("uniwrt.db"), Arc::clone(&clock))
            .await
            .unwrap();
        let sim = SimFleet::spawn(
            &SimConfig {
                reboot_every: None,
                ..SimConfig::default()
            },
            clock.as_ref(),
        );
        let engine = Engine::new(EngineConfig::default(), Arc::clone(&clock));
        let mut ids = Vec::new();
        for (index, node) in sim.nodes().into_iter().enumerate() {
            let adopted = store
                .create_node(NewNode {
                    name: node.name,
                    mac: node.mac,
                    board_name: node.board_name,
                    host: node.host,
                    port: 22,
                    role: node.role,
                })
                .await
                .unwrap();
            engine
                .add(
                    adopted.id,
                    (&adopted).into(),
                    sim.link(index).unwrap(),
                    None,
                )
                .await;
            ids.push(adopted.id);
        }
        let fleet = Arc::new(engine.clone());
        let vault = Vault::open(&store, &MasterKey::generate().unwrap())
            .await
            .unwrap();
        let notifications = Notifications::start(
            &engine,
            store.clone(),
            vault.clone(),
            Sender::new("uniwrt/test").unwrap(),
            NotifyConfig::default(),
        )
        .await;
        let alerts =
            AlertEngine::start(&engine, store.clone(), None, AlertsConfig::default()).await;
        let dial: Option<Arc<dyn Dial>> = dial.then(|| {
            Arc::new(SshDial {
                config: SshConfig {
                    connect_timeout: Duration::from_secs(2),
                    ..SshConfig::default()
                },
            }) as Arc<dyn Dial>
        });
        let notify = Some((notifications.handle(), vault));
        let (rf, drift) = scanners(&engine, &store).await;
        let api = api_full(
            store.clone(),
            fleet,
            clock,
            dial,
            Some(alerts.handle()),
            notify,
            Some(rf.handle()),
            Some(drift.handle()),
        )
        .await;
        api.create_admin("admin", PASSWORD).await.unwrap();
        let mut demo = Self {
            _dir: dir,
            store,
            engine,
            _sim: sim,
            api,
            cookie: String::new(),
            ids,
            logs: None,
            _alerts: alerts,
            _notifications: notifications,
            _rf: rf,
            _drift: drift,
        };
        demo.cookie = login(&demo.api).await;
        demo
    }

    /// Stores the nodes' logs from now on (their agents follow them).
    pub(crate) async fn record_logs(&mut self) {
        let recorder = LogRecorder::start(
            &self.engine,
            self.store.clone(),
            None,
            LogsConfig::default(),
        )
        .await;
        self.logs = Some(recorder);
    }

    pub(crate) fn router(&self) -> Router {
        router(self.api.clone())
    }

    /// Sends a request through the router, as the HTTPS listener hands it over.
    pub(crate) async fn send(&self, request: Request) -> Response {
        router_send(&self.api, request).await
    }

    pub(crate) fn get(&self, uri: &str) -> Request {
        Request::get(uri)
            .header(header::COOKIE, &self.cookie)
            .body(Body::empty())
            .unwrap()
    }

    /// Serves the router on a loopback port, as the HTTPS listener would (without TLS).
    pub(crate) async fn serve(&self) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = self
            .router()
            .layer(middleware::from_fn(|request: Request, next: Next| {
                next.run(with_client(request))
            }));
        tokio::spawn(async move { axum::serve(listener, app).await });
        addr
    }
}

/// RF insight scanning the simulated radios at once, one after another, busy or not; drift
/// detection reading what the store confirmed every 100 ms.
async fn scanners(engine: &Engine<SimLink>, store: &Store) -> (RfScanner, DriftWatch) {
    let rf_config = RfConfig {
        tick_every: Duration::from_millis(20),
        first_delay: Duration::ZERO,
        stagger: Duration::ZERO,
        gap: Duration::ZERO,
        idle_kbps: u64::MAX,
        rebuild_after: Duration::ZERO,
        ..RfConfig::default()
    };
    let rf = RfScanner::start(engine, store.clone(), rf_config).await;
    let drift_config = DriftConfig {
        tick_every: Duration::from_millis(100),
        ..DriftConfig::default()
    };
    let drift = DriftWatch::start(engine, store.clone(), None, drift_config)
        .await
        .unwrap();
    (rf, drift)
}

pub(crate) fn post(uri: &str, body: &Value, cookie: Option<&str>) -> Request {
    request(Method::POST, uri, Some(body), cookie)
}

/// A state-changing request, as the UI sends it, with a JSON body or none.
pub(crate) fn request(
    method: Method,
    uri: &str,
    body: Option<&Value>,
    cookie: Option<&str>,
) -> Request {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-uniwrt-csrf", "1")
        .header(header::ORIGIN, ORIGIN);
    if body.is_some() {
        request = request.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    request.body(body).unwrap()
}

fn with_client(mut request: Request) -> Request {
    request.extensions_mut().insert(Client {
        ip: "192.168.1.20".parse().unwrap(),
        secure: true,
        host: Some(HOST.into()),
    });
    request
}

pub(crate) async fn json(res: Response) -> Value {
    let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
