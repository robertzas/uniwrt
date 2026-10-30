//! The HTTP surface: the API (`uniwrt-api`) under `/api`, the embedded UI as the fallback, and
//! the middleware both share. The HTTPS listener serves [`https`] around [`router`], the plain
//! HTTP listener [`plain::router`]; each tells the API who the client is ([`Client`]).

pub(crate) mod plain;

use axum::{
    Router,
    extract::{ConnectInfo, Request},
    http::{HeaderMap, Method, Uri, header},
    middleware::{self, Next},
};
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};
use uniwrt_api::{Api, Client, security};

use crate::{
    tls::listener::Peer,
    ui::{self, Assets},
};

/// The application router. `assets` is the embedded UI (`ui::EMBEDDED`, or a table in tests).
pub(crate) fn router(assets: &'static Assets, api: Api) -> Router {
    Router::new()
        .nest("/api", uniwrt_api::router(api))
        .fallback(
            move |method: Method, uri: Uri, headers: HeaderMap| async move {
                let path = uri.path();
                // `nest` leaves `/api/` (an empty tail) to this fallback; it is still an API miss.
                if path == "/api" || path.starts_with("/api/") {
                    return uniwrt_api::no_route(path);
                }
                ui::respond(assets, &method, path, &headers)
            },
        )
        // Innermost first: panics become 500s that still get security headers and a trace.
        .layer(CatchPanicLayer::new())
        .layer(middleware::map_response(security::headers))
        .layer(TraceLayer::new_for_http())
}

/// `app` as the HTTPS listener serves it: the client is the connection's peer, over HTTPS.
pub(crate) fn https(app: Router) -> Router {
    app.layer(middleware::from_fn(
        |ConnectInfo(Peer(peer)): ConnectInfo<Peer>, mut request: Request, next: Next| async move {
            let client = Client {
                ip: peer.ip().to_canonical(),
                secure: true,
                host: host(&request),
            };
            request.extensions_mut().insert(client);
            next.run(request).await
        },
    ))
}

/// The host the client addressed: the `Host` header, or HTTP/2's `:authority`.
pub(crate) fn host(request: &Request) -> Option<String> {
    request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().authority().map(ToString::to_string))
}

/// An API over a fresh store, for the server's own tests.
#[cfg(test)]
pub(crate) async fn test_api() -> (tempfile::TempDir, Api) {
    use std::sync::Arc;

    use uniwrt_api::{ApiConfig, PasswordCost};
    use uniwrt_core::SystemClock;
    use uniwrt_engine::{Engine, EngineConfig};

    use crate::fleet_link::FleetLink;

    let dir = tempfile::tempdir().unwrap();
    let store = uniwrt_store::Store::open(dir.path().join("uniwrt.db"), Arc::new(SystemClock))
        .await
        .unwrap();
    let engine = Engine::<FleetLink>::new(EngineConfig::default(), Arc::new(SystemClock));
    let key = uniwrt_node::ControllerKey::generate().unwrap();
    let jobs = uniwrt_jobs::Jobs::start(uniwrt_jobs::JobsConfig {
        store: store.clone(),
        vault: uniwrt_store::Vault::open(&store, &uniwrt_store::MasterKey::generate().unwrap())
            .await
            .unwrap(),
        clock: Arc::new(SystemClock),
        key: key.clone(),
        dial: None,
        watch: Arc::new(crate::fleet::Watcher {
            engine: engine.clone(),
            key,
            clock: Arc::new(SystemClock),
        }),
    })
    .await
    .unwrap();
    let config = ApiConfig {
        password_cost: PasswordCost::CHEAPEST,
        ..ApiConfig::new(
            store,
            Arc::new(engine),
            Arc::new(SystemClock),
            crate::build_info::API,
            Arc::new(crate::tls::Tls::for_tests()),
            jobs,
        )
    };
    (dir, Api::new(config).await.unwrap())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{
        body::{self, Body, Bytes},
        extract::ConnectInfo,
        http::{Request, StatusCode},
        response::Response,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    use super::{https, router, test_api};
    use crate::{build_info, tls::listener::Peer, ui::EMBEDDED};

    /// Sends a request as the HTTPS listener would, from 192.168.1.20.
    async fn send(method: &str, uri: &str, headers: &[(&str, &str)]) -> Response {
        let (_dir, api) = test_api().await;
        let mut request = Request::builder().method(method).uri(uri);
        for &(name, value) in headers {
            request = request.header(name, value);
        }
        let mut request = request.body(Body::empty()).unwrap();
        let peer: SocketAddr = "192.168.1.20:50000".parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(Peer(peer)));
        https(router(&EMBEDDED, api))
            .oneshot(request)
            .await
            .unwrap()
    }

    fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().unwrap())
    }

    async fn body(response: Response) -> Bytes {
        body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn the_api_is_under_api_with_the_build_identity() {
        let res = send("GET", "/api/v1/health", &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(header(&res, "content-type"), Some("application/json"));
        assert_eq!(header(&res, "cache-control"), Some("no-store"));
        let json: Value = serde_json::from_slice(&body(res).await).unwrap();
        assert_eq!(
            json,
            json!({
                "status": "ok",
                "version": build_info::VERSION,
                "commit": build_info::COMMIT,
                "built_at": build_info::BUILT_AT,
            })
        );
    }

    #[tokio::test]
    async fn the_https_listener_identifies_the_client_to_the_api() {
        let headers = [
            ("host", "uniwrt.lan:8443"),
            ("origin", "https://uniwrt.lan:8443"),
            ("x-uniwrt-csrf", "1"),
        ];
        // Logout needs the client (for the audit) and checks the origin against it.
        let res = send("POST", "/api/v1/auth/logout", &headers).await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        let mut other = headers;
        other[1] = ("origin", "https://other.lan:8443");
        let res = send("POST", "/api/v1/auth/logout", &other).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unknown_api_paths_are_problem_json_not_the_spa() {
        for path in ["/api/v1/nope", "/api", "/api/"] {
            let res = send("GET", path, &[("accept", "text/html")]).await;
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
            assert_eq!(
                header(&res, "content-type"),
                Some("application/problem+json"),
                "{path}"
            );
            let json: Value = serde_json::from_slice(&body(res).await).unwrap();
            assert_eq!(json["status"], 404);
            assert_eq!(json["title"], "Not Found");
            assert_eq!(json["detail"], format!("No API route matches {path}"));
        }
    }

    #[tokio::test]
    async fn the_ui_is_the_fallback_and_carries_security_headers() {
        let res = send("GET", "/nodes/gateway", &[("accept", "text/html")]).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            header(&res, "content-type"),
            Some("text/html; charset=utf-8")
        );
        assert!(
            header(&res, "content-security-policy")
                .is_some_and(|csp| csp.contains("default-src 'self'"))
        );
        assert_eq!(header(&res, "x-content-type-options"), Some("nosniff"));
        assert_eq!(header(&res, "x-frame-options"), Some("DENY"));
        assert_eq!(body(res).await, EMBEDDED.index().identity.bytes);

        let res = send("GET", "/api/v1/health", &[]).await;
        assert_eq!(header(&res, "referrer-policy"), Some("no-referrer"));
    }

    #[tokio::test]
    async fn head_has_headers_but_no_body() {
        let res = send("HEAD", "/", &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        let length = EMBEDDED.index().identity.bytes.len().to_string();
        assert_eq!(header(&res, "content-length"), Some(length.as_str()));
        assert!(body(res).await.is_empty());
    }
}
