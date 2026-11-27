//! Users through the router: admins add, list, change and delete them, keeping an admin; a
//! change ends the user's sessions; everyone changes their own password with the current one.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use serde_json::{Value, json};
use uniwrt_core::{ManualClock, Timestamp};
use uniwrt_engine::{Engine, EngineConfig};
use uniwrt_model::user::AuditAction;
use uniwrt_sim::SimLink;
use uniwrt_store::{AuditRepo as _, Store};

use crate::{
    Api,
    testing::{self, PASSWORD, json, post, request, router_send},
};

const START: Timestamp = Timestamp::from_unix_secs(1_793_318_400);
const BOB: &str = "bob's first password";

struct Harness {
    _dir: tempfile::TempDir,
    store: Store,
    api: Api,
    /// The admin's session cookie.
    admin: String,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(START);
    let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(clock.clone()))
        .await
        .unwrap();
    let fleet = Engine::<SimLink>::new(EngineConfig::default(), Arc::new(clock.clone()));
    let api = testing::api(store.clone(), Arc::new(fleet), Arc::new(clock)).await;
    let setup = json!({ "username": "admin", "password": PASSWORD });
    let res = router_send(&api, post("/v1/auth/setup", &setup, None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let admin = cookie(&res).unwrap();
    Harness {
        _dir: dir,
        store,
        api,
        admin,
    }
}

fn cookie(res: &Response) -> Option<String> {
    let set = res.headers().get(header::SET_COOKIE)?.to_str().unwrap();
    let pair = set.split(';').next()?;
    (!pair.ends_with('=')).then(|| pair.to_owned())
}

impl Harness {
    async fn send(&self, method: Method, uri: &str, body: Option<Value>, cookie: &str) -> Response {
        router_send(&self.api, request(method, uri, body.as_ref(), Some(cookie))).await
    }

    async fn get(&self, uri: &str, cookie: &str) -> Response {
        let request = Request::get(uri)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        router_send(&self.api, request).await
    }

    /// Logs `username` in: the cookie, or the refusal's status.
    async fn login(&self, username: &str, password: &str) -> Result<String, StatusCode> {
        let body = json!({ "username": username, "password": password });
        let res = router_send(&self.api, post("/v1/auth/login", &body, None)).await;
        if res.status() == StatusCode::OK {
            Ok(cookie(&res).unwrap())
        } else {
            Err(res.status())
        }
    }

    async fn alive(&self, cookie: &str) -> bool {
        self.get("/v1/auth/me", cookie).await.status() == StatusCode::OK
    }

    async fn details(&self, action: AuditAction) -> Vec<String> {
        let mut log = self.store.audit_log(100).await.unwrap();
        log.reverse();
        log.into_iter()
            .filter(|entry| entry.action == action)
            .filter_map(|entry| entry.detail)
            .collect()
    }
}

async fn detail(res: Response) -> String {
    json(res).await["detail"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn admins_add_change_and_delete_users_keeping_an_admin() {
    let h = harness().await;
    let add = json!({ "username": " Bob ", "password": BOB, "role": "viewer" });
    let res = h
        .send(Method::POST, "/v1/users", Some(add.clone()), &h.admin)
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let bob = json(res).await;
    assert_eq!(
        (bob["username"].clone(), bob["role"].clone()),
        (json!("bob"), json!("viewer"))
    );
    let id = bob["id"].as_i64().unwrap();
    let res = h.send(Method::POST, "/v1/users", Some(add), &h.admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(detail(res).await, "A user named bob already exists");
    let short = json!({ "username": "carol", "password": "short", "role": "admin" });
    let res = h
        .send(Method::POST, "/v1/users", Some(short), &h.admin)
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let list = json(h.get("/v1/users", &h.admin).await).await;
    let names: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["username"].clone())
        .collect();
    assert_eq!(names, [json!("admin"), json!("bob")]);
    assert!(list[0]["last_seen_at"].is_number());
    assert!(list[1]["last_seen_at"].is_null());

    // A viewer neither lists nor changes users.
    let viewer = h.login("bob", BOB).await.unwrap();
    assert_eq!(
        h.get("/v1/users", &viewer).await.status(),
        StatusCode::FORBIDDEN
    );
    let promote = json!({ "role": "admin" });
    let uri = format!("/v1/users/{id}");
    let res = h
        .send(Method::PATCH, &uri, Some(promote.clone()), &viewer)
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // A role change ends the user's sessions.
    let res = h.send(Method::PATCH, &uri, Some(promote), &h.admin).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json(res).await["role"], "admin");
    assert!(!h.alive(&viewer).await);
    assert!(h.alive(&h.admin).await);

    // So does a new password, which the old one no longer opens.
    let bob_session = h.login("bob", BOB).await.unwrap();
    let reset = json!({ "password": "bob's second password" });
    let res = h.send(Method::PATCH, &uri, Some(reset), &h.admin).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(!h.alive(&bob_session).await);
    assert_eq!(h.login("bob", BOB).await, Err(StatusCode::UNAUTHORIZED));
    h.login("bob", "bob's second password").await.unwrap();
    assert_eq!(
        h.details(AuditAction::Users).await,
        [
            "added bob (viewer)",
            "bob: viewer → admin; sessions ended",
            "bob: password set; sessions ended",
        ]
    );
}

#[tokio::test]
async fn admins_keep_an_admin_and_their_own_account() {
    let h = harness().await;
    let add = json!({ "username": "bob", "password": BOB, "role": "admin" });
    let res = h.send(Method::POST, "/v1/users", Some(add), &h.admin).await;
    let id = json(res).await["id"].as_i64().unwrap();
    let uri = format!("/v1/users/{id}");
    let bob_session = h.login("bob", BOB).await.unwrap();

    // No admin sets their own password here, or deletes themselves.
    let own = format!("/v1/users/{}", 1);
    let res = h
        .send(
            Method::PATCH,
            &own,
            Some(json!({ "password": "a new one for me" })),
            &h.admin,
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = h.send(Method::DELETE, &own, None, &h.admin).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // Two admins: the first may step down, keeping their session; then bob is the last.
    let res = h
        .send(
            Method::PATCH,
            &own,
            Some(json!({ "role": "viewer" })),
            &h.admin,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(h.alive(&h.admin).await, "the asking session stays");
    let res = h
        .send(
            Method::PATCH,
            &uri,
            Some(json!({ "role": "viewer" })),
            &bob_session,
        )
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        detail(res).await,
        "bob is the last admin: make another user an admin first"
    );
    let res = h
        .send(
            Method::PATCH,
            &own,
            Some(json!({ "role": "admin" })),
            &bob_session,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        !h.alive(&h.admin).await,
        "the promoted admin's sessions ended"
    );

    let admin = h.login("admin", PASSWORD).await.unwrap();
    let res = h.send(Method::DELETE, &uri, None, &admin).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(!h.alive(&bob_session).await);
    let res = h.send(Method::DELETE, &uri, None, &admin).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    assert_eq!(
        h.details(AuditAction::Users).await,
        [
            "added bob (admin)",
            "admin: admin → viewer; sessions ended",
            "admin: viewer → admin; sessions ended",
            "deleted bob",
        ]
    );
}

#[tokio::test]
async fn everyone_changes_their_own_password_with_the_current_one() {
    let h = harness().await;
    let add = json!({ "username": "bob", "password": BOB, "role": "viewer" });
    h.send(Method::POST, "/v1/users", Some(add), &h.admin).await;
    let laptop = h.login("bob", BOB).await.unwrap();
    let phone = h.login("bob", BOB).await.unwrap();

    let change = |current: &str, new: &str| Some(json!({ "current": current, "new": new }));
    let res = h
        .send(
            Method::POST,
            "/v1/auth/password",
            change("wrong one", "a long new password"),
            &laptop,
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(detail(res).await, "The current password is wrong");
    assert!(
        h.alive(&laptop).await,
        "a wrong password doesn't end the session"
    );
    let res = h
        .send(
            Method::POST,
            "/v1/auth/password",
            change(BOB, "short"),
            &laptop,
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let res = h
        .send(
            Method::POST,
            "/v1/auth/password",
            change(BOB, "a long new password"),
            &laptop,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert!(h.alive(&laptop).await);
    assert!(!h.alive(&phone).await);
    assert!(h.alive(&h.admin).await, "others' sessions stay");
    assert_eq!(h.login("bob", BOB).await, Err(StatusCode::UNAUTHORIZED));
    h.login("bob", "a long new password").await.unwrap();
    assert_eq!(
        h.details(AuditAction::Password).await,
        [
            "refused: the current password was wrong",
            "changed; other sessions ended"
        ]
    );
}
