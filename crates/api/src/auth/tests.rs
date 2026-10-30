//! The auth routes through the router, over a temporary store: setup and its window, login and
//! its throttling, sessions and their end, logout, the audit log, and the CSRF checks.

use std::{sync::Arc, time::Duration};

use axum::{
    Router,
    body::{self, Body},
    http::{Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_model::user::AuditAction;
use uniwrt_sim::SimLink;
use uniwrt_store::{AuditRepo as _, Store};

use crate::{
    Client, SETUP_WINDOW,
    auth::session::IDLE,
    router,
    testing::{self, PASSWORD},
};

const START: Timestamp = Timestamp::from_unix_secs(1_793_318_400);

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    clock: ManualClock,
    router: Router,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(clock.clone()))
        .await
        .unwrap();
    let fleet = Engine::<SimLink>::new(EngineConfig::default(), Arc::new(clock.clone()));
    let api = testing::api(store.clone(), Arc::new(fleet), Arc::new(clock.clone())).await;
    Harness {
        _dir: dir,
        store,
        clock,
        router: router(api),
    }
}

/// A request as the HTTPS listener hands it over, from 192.168.1.20.
struct Req {
    method: &'static str,
    uri: &'static str,
    body: Option<Value>,
    cookie: Option<String>,
    csrf: bool,
    origin: Option<&'static str>,
}

fn get(uri: &'static str) -> Req {
    Req {
        method: "GET",
        uri,
        body: None,
        cookie: None,
        csrf: false,
        origin: None,
    }
}

fn post(uri: &'static str, body: Option<Value>) -> Req {
    Req {
        method: "POST",
        uri,
        body,
        cookie: None,
        csrf: true,
        origin: Some("https://uniwrt.lan:8443"),
    }
}

impl Req {
    fn cookie(mut self, cookie: &str) -> Self {
        self.cookie = Some(cookie.to_owned());
        self
    }
}

impl Harness {
    async fn send(&self, req: Req) -> Response {
        let mut request = Request::builder().method(req.method).uri(req.uri);
        if let Some(cookie) = &req.cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if req.csrf {
            request = request.header("x-uniwrt-csrf", "1");
        }
        if let Some(origin) = req.origin {
            request = request.header(header::ORIGIN, origin);
        }
        let body = match req.body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let mut request = request.body(body).unwrap();
        request.extensions_mut().insert(Client {
            ip: "192.168.1.20".parse().unwrap(),
            secure: true,
            host: Some("uniwrt.lan:8443".into()),
        });
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// Runs setup as `admin` and returns the session cookie (`name=value`).
    async fn set_up(&self) -> String {
        let res = self
            .send(post("/v1/auth/setup", Some(credentials("admin", PASSWORD))))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        session_cookie(&res).unwrap()
    }

    async fn login(&self, username: &str, password: &str) -> Response {
        self.send(post(
            "/v1/auth/login",
            Some(credentials(username, password)),
        ))
        .await
    }

    async fn audit(&self) -> Vec<(AuditAction, Option<String>)> {
        let mut log = self.store.audit_log(100).await.unwrap();
        log.reverse();
        log.into_iter()
            .map(|entry| (entry.action, entry.username))
            .collect()
    }
}

fn credentials(username: &str, password: &str) -> Value {
    json!({ "username": username, "password": password })
}

fn session_cookie(res: &Response) -> Option<String> {
    let set = res.headers().get(header::SET_COOKIE)?.to_str().unwrap();
    let pair = set.split(';').next()?;
    (!pair.ends_with('=')).then(|| pair.to_owned())
}

fn clears_cookie(res: &Response) -> bool {
    res.headers()
        .get(header::SET_COOKIE)
        .is_some_and(|set| set.to_str().unwrap().contains("Max-Age=0"))
}

async fn json(res: Response) -> Value {
    let bytes = body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn setup_creates_the_admin_once_and_logs_them_in() {
    let h = harness().await;
    let state = json(h.send(get("/v1/auth")).await).await;
    assert_eq!(state["setup"], "open");
    assert_eq!(state["setup_closes_in_s"], 600);
    assert_eq!(state["user"], Value::Null);
    assert_eq!(state["min_password_chars"], 12);

    let res = h
        .send(post(
            "/v1/auth/setup",
            Some(credentials(" Admin ", PASSWORD)),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).unwrap();
    assert!(cookie.starts_with("__Host-uniwrt-session="));
    assert_eq!(
        json(res).await,
        json!({ "id": 1, "username": "admin", "role": "admin" })
    );

    let res = h.send(get("/v1/auth/me").cookie(&cookie)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["username"], "admin");
    let state = json(h.send(get("/v1/auth").cookie(&cookie)).await).await;
    assert_eq!(
        (state["setup"].clone(), state["user"]["username"].clone()),
        (json!("done"), json!("admin"))
    );

    let res = h
        .send(post("/v1/auth/setup", Some(credentials("other", PASSWORD))))
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        h.audit().await,
        [(AuditAction::Setup, Some("admin".into()))]
    );
}

#[tokio::test]
async fn setup_checks_the_username_and_the_password() {
    let h = harness().await;
    for (username, password) in [("admin", "short"), ("not valid", PASSWORD)] {
        let res = h
            .send(post(
                "/v1/auth/setup",
                Some(credentials(username, password)),
            ))
            .await;
        assert_eq!(
            res.status(),
            StatusCode::BAD_REQUEST,
            "{username} {password}"
        );
        assert_eq!(
            res.headers()[header::CONTENT_TYPE],
            "application/problem+json"
        );
    }
    let res = h
        .send(post("/v1/auth/setup", Some(json!({ "username": "admin" }))))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(h.send(get("/v1/auth")).await).await["setup"], "open");
}

#[tokio::test(start_paused = true)]
async fn setup_closes_ten_minutes_after_start() {
    let h = harness().await;
    tokio::time::advance(SETUP_WINDOW).await;
    assert_eq!(json(h.send(get("/v1/auth")).await).await["setup"], "closed");
    let res = h
        .send(post("/v1/auth/setup", Some(credentials("admin", PASSWORD))))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(
        json(res).await["detail"]
            .as_str()
            .unwrap()
            .contains("restart")
    );
}

#[tokio::test]
async fn login_logout_and_the_audit_log() {
    let h = harness().await;
    h.set_up().await;

    let res = h.login("admin", "wrong password!").await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(clears_cookie(&res));
    let res = h.login("nobody", PASSWORD).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(res).await["detail"], "Wrong username or password");

    let res = h.login("ADMIN", PASSWORD).await;
    assert_eq!(res.status(), StatusCode::OK);
    let cookie = session_cookie(&res).unwrap();
    assert_eq!(
        h.send(get("/v1/auth/me").cookie(&cookie)).await.status(),
        StatusCode::OK
    );

    let res = h.send(post("/v1/auth/logout", None).cookie(&cookie)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(clears_cookie(&res));
    assert_eq!(
        h.send(get("/v1/auth/me").cookie(&cookie)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    // Logging out again, or without a session, is harmless.
    assert_eq!(
        h.send(post("/v1/auth/logout", None).cookie(&cookie))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        h.audit().await,
        [
            (AuditAction::Setup, Some("admin".into())),
            (AuditAction::LoginFailed, Some("admin".into())),
            (AuditAction::LoginFailed, Some("nobody".into())),
            (AuditAction::Login, Some("admin".into())),
            (AuditAction::Logout, Some("admin".into())),
        ]
    );
    let entry = &h.store.audit_log(1).await.unwrap()[0];
    assert_eq!(entry.ip.as_deref(), Some("192.168.1.20"));
}

#[tokio::test]
async fn repeated_failures_make_the_username_wait() {
    let h = harness().await;
    h.set_up().await;
    for _ in 0..5 {
        assert_eq!(
            h.login("admin", "wrong password!").await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    // Even the right password waits now.
    let res = h.login("admin", PASSWORD).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.headers()[header::RETRY_AFTER], "60");
}

#[tokio::test]
async fn sessions_end_when_idle_and_bad_cookies_are_cleared() {
    let h = harness().await;
    let cookie = h.set_up().await;
    h.clock.advance(IDLE.saturating_sub(Duration::from_secs(1)));
    assert_eq!(
        h.send(get("/v1/auth/me").cookie(&cookie)).await.status(),
        StatusCode::OK
    );
    // That request was recent use: the idle time starts again.
    h.clock.advance(IDLE.saturating_sub(Duration::from_secs(1)));
    assert_eq!(
        h.send(get("/v1/auth/me").cookie(&cookie)).await.status(),
        StatusCode::OK
    );
    h.clock.advance(IDLE);
    let res = h.send(get("/v1/auth/me").cookie(&cookie)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert!(clears_cookie(&res));

    for cookie in ["__Host-uniwrt-session=forged", "other=1"] {
        let res = h.send(get("/v1/auth/me").cookie(cookie)).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{cookie}");
    }
    assert_eq!(
        h.send(get("/v1/auth/me")).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn state_changing_requests_need_the_csrf_header_and_our_origin() {
    let h = harness().await;
    let setup = || post("/v1/auth/setup", Some(credentials("admin", PASSWORD)));
    let res = h
        .send(Req {
            csrf: false,
            ..setup()
        })
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(
        json(res).await["detail"]
            .as_str()
            .unwrap()
            .contains("x-uniwrt-csrf")
    );
    for origin in ["https://evil.example", "http://uniwrt.lan:8443", "null"] {
        let res = h
            .send(Req {
                origin: Some(origin),
                ..setup()
            })
            .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{origin}");
    }
    // Without an Origin (a script, not a browser), the header suffices.
    assert_eq!(
        h.send(Req {
            origin: None,
            ..setup()
        })
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn public_routes_and_problem_fallbacks() {
    let h = harness().await;
    let res = h.send(get("/v1/health")).await;
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        json(res).await,
        json!({ "status": "ok", "version": "1.2.3", "commit": "abc", "built_at": "2026-10-30T00:00:00Z" })
    );
    assert_eq!(
        json(h.send(get("/v1/tls")).await).await["names"],
        json!(["uniwrt.lan"])
    );
    let res = h.send(get("/v1/tls/ca.crt")).await;
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "application/x-x509-ca-cert"
    );

    let res = h.send(get("/v1/nope")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let res = h.send(post("/v1/health", None)).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert!(
        res.headers()[header::ALLOW]
            .to_str()
            .unwrap()
            .contains("GET")
    );
}
