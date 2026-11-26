//! The route table against the roles layer: every route the source declares is reached
//! through it, a viewer is refused every write and admin read before the handler runs, and
//! nothing but the public routes answers without a session.

use std::{path::Path, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{self, Body},
    http::{Method, Request, StatusCode, header},
};
use ring::digest;
use tower::ServiceExt as _;
use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_model::user::Role;
use uniwrt_sim::SimLink;
use uniwrt_store::{NewSession, SessionRepo as _, Store, UserRepo as _};

use super::{ADMIN_READS, Access, PUBLIC, REFUSAL, VIEWER_WRITES, access};
use crate::{
    auth::session::COOKIE,
    router,
    testing::{self, ORIGIN, with_client},
};

const START: Timestamp = Timestamp::from_unix_secs(1_793_318_400);
const TOKEN: &str = "viewer-session-token";
const METHODS: [&str; 6] = ["get", "post", "put", "delete", "patch", "connect"];

/// `(METHOD, path)` of every `.route(…)` under `src/` outside tests, read from the source.
fn route_table() -> Vec<(String, String)> {
    let mut routes = Vec::new();
    let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            if path.is_dir() {
                if name != "tests" {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && !name.ends_with("tests.rs")
                && !name.starts_with("testing")
            {
                routes.extend(routes_in(&std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    routes.sort();
    routes.dedup();
    routes
}

fn routes_in(source: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(".route(") {
        rest = &rest[at + ".route(".len()..];
        let Some(path) = rest
            .trim_start()
            .strip_prefix('"')
            .and_then(|s| s.split('"').next())
        else {
            continue;
        };
        // The call's arguments, up to its closing parenthesis.
        let mut depth = 1;
        let end = rest
            .char_indices()
            .find(|&(_, c)| {
                depth += match c {
                    '(' => 1,
                    ')' => -1,
                    _ => 0,
                };
                depth == 0
            })
            .map_or(rest.len(), |(i, _)| i);
        let args = &rest[..end];
        for method in METHODS {
            let call = format!("{method}(");
            let mut from = 0;
            while let Some(i) = args[from..].find(&call) {
                let i = from + i;
                let before = args[..i].chars().next_back();
                if before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':')) {
                    found.push((method.to_uppercase(), path.to_owned()));
                }
                from = i + call.len();
            }
        }
    }
    found
}

/// The table's API routes, which `router` serves (`/b/…` and `/feeds/…` are the server's).
fn api_routes() -> Vec<(String, String)> {
    route_table()
        .into_iter()
        .filter(|(_, path)| path.starts_with("/v1/"))
        .collect()
}

/// A concrete URI for a route's pattern.
fn uri(pattern: &str) -> String {
    pattern
        .split('/')
        .map(|segment| match segment {
            "{mac}" => "02:00:00:00:00:01",
            "{port}" | "{peer_port}" => "lan1",
            "{package}" => "network",
            "{vertex}" => "node-1",
            "{hash}" | "{sha256}" | "{token}" => "00ff",
            "{name}" => "backup.age",
            s if s.starts_with('{') => "1",
            s => s,
        })
        .collect::<Vec<_>>()
        .join("/")
}

struct Harness {
    _dir: tempfile::TempDir,
    router: Router,
}

/// A controller whose only user is a viewer, with a session `TOKEN`.
async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(clock.clone()))
        .await
        .unwrap();
    let viewer = store
        .create_first_user("viewer", Role::Viewer, "not-a-hash")
        .await
        .unwrap();
    let digest = digest::digest(&digest::SHA256, TOKEN.as_bytes());
    store
        .create_session(NewSession {
            token_sha256: digest.as_ref().try_into().unwrap(),
            user: viewer.id,
            expires_at: START.saturating_add(Duration::from_hours(24)),
            ip: None,
            user_agent: None,
        })
        .await
        .unwrap();
    let fleet = Engine::<SimLink>::new(EngineConfig::default(), Arc::new(clock.clone()));
    let api = testing::api(store, Arc::new(fleet), Arc::new(clock)).await;
    Harness {
        _dir: dir,
        router: router(api),
    }
}

/// The status and problem detail of `method` on `pattern`, as the UI would send it; `None`
/// when the handler is still busy after a few seconds (it ran: the layer let it through).
async fn send(
    harness: &Harness,
    method: &str,
    pattern: &str,
    session: bool,
) -> Option<(StatusCode, String)> {
    let mut request = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).unwrap())
        .uri(uri(pattern))
        .header("x-uniwrt-csrf", "1")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json");
    if session {
        request = request.header(header::COOKIE, format!("{COOKIE}={TOKEN}"));
    }
    let request = with_client(request.body(Body::from("{}")).unwrap());
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        harness.router.clone().oneshot(request),
    )
    .await
    .ok()?
    .unwrap();
    let status = response.status();
    let bytes = body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap_or_default();
    let detail = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|body| body["detail"].as_str().map(str::to_owned))
        .unwrap_or_default();
    Some((status, detail))
}

#[test]
fn the_route_table_is_read_and_the_lists_name_routes_in_it() {
    let table = route_table();
    assert!(
        table.len() > 150,
        "{} routes: is the scan broken?",
        table.len()
    );
    let has = |method: &str, path: &str| table.iter().any(|(m, p)| m == method && p == path);
    for (method, path) in PUBLIC.iter().chain(VIEWER_WRITES) {
        assert!(has(method, path), "{method} {path} is no route");
    }
    for path in ADMIN_READS {
        assert!(has("GET", path), "GET {path} is no route");
    }
}

#[test]
fn writes_default_to_admins_and_the_threat_model_s_reads_are_theirs() {
    for (method, path) in [
        (Method::POST, "/v1/adopt/probe"),
        (Method::POST, "/v1/site/secrets/reveal"),
        (Method::POST, "/v1/nodes/{id}/root-password/reveal"),
        (Method::POST, "/v1/vpn/wireguard/peers/{id}/config"),
        (Method::POST, "/v1/a/route/added/tomorrow"),
        (Method::PATCH, "/v1/clients/{mac}"),
        (Method::GET, "/v1/nodes/{id}/terminal"),
        (Method::CONNECT, "/v1/nodes/{id}/terminal"),
        (Method::GET, "/v1/nodes/{id}/diag/capture"),
        (Method::GET, "/v1/support/bundle"),
    ] {
        assert_eq!(access(&method, path), Access::Admin, "{method} {path}");
    }
    assert_eq!(access(&Method::GET, "/v1/nodes"), Access::Viewer);
    assert_eq!(access(&Method::CONNECT, "/v1/ws"), Access::Viewer);
    assert_eq!(access(&Method::POST, "/v1/auth/login"), Access::Public);
    assert_eq!(
        access(&Method::POST, "/v1/controller/restore/apply"),
        Access::AdminOrSetup
    );
}

#[tokio::test]
async fn a_viewer_is_refused_every_write_and_admin_read_before_its_handler() {
    let harness = harness().await;
    let mut wrong = Vec::new();
    // Logging out would end the session the sweep uses.
    for (method, path) in api_routes()
        .into_iter()
        .filter(|(_, path)| path != "/v1/auth/logout")
    {
        let expected = access(&Method::from_bytes(method.as_bytes()).unwrap(), &path);
        let answer = send(&harness, &method, &path, true).await;
        let refused = answer
            .as_ref()
            .is_some_and(|(status, detail)| *status == StatusCode::FORBIDDEN && detail == REFUSAL);
        let admins = matches!(expected, Access::Admin | Access::AdminOrSetup);
        if refused != admins {
            wrong.push(format!("{method} {path}: {expected:?}, got {answer:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test]
async fn only_the_public_routes_answer_without_a_session() {
    let harness = harness().await;
    let mut wrong = Vec::new();
    for (method, path) in api_routes() {
        let expected = access(&Method::from_bytes(method.as_bytes()).unwrap(), &path);
        if expected == Access::Public {
            continue;
        }
        let answer = send(&harness, &method, &path, false).await;
        if answer
            .as_ref()
            .is_none_or(|(status, _)| *status != StatusCode::UNAUTHORIZED)
        {
            wrong.push(format!("{method} {path}: {answer:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
