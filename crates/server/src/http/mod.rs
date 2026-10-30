//! The HTTP surface: `/api` routes, the embedded UI as the fallback, and the middleware both share;
//! the HTTPS listener serves [`router`], the plain HTTP listener [`plain::router`] around it.

mod health;
pub(crate) mod plain;
mod problem;
mod security;
mod tls;

use axum::{
    Router,
    http::{HeaderMap, Method, Uri},
    middleware,
    routing::get,
};
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};

use crate::{
    tls::Tls,
    ui::{self, Assets},
};

/// The application router. `assets` is the embedded UI (`ui::EMBEDDED`, or a table in tests).
pub(crate) fn router(assets: &'static Assets, tls: Tls) -> Router {
    let api = Router::new()
        .route("/v1/health", get(health::get))
        .route("/v1/tls", get(tls::info))
        .route("/v1/tls/ca.crt", get(tls::ca))
        .fallback(problem::not_found)
        .method_not_allowed_fallback(problem::method_not_allowed)
        .with_state(tls);
    Router::new()
        .nest("/api", api)
        .fallback(
            move |method: Method, uri: Uri, headers: HeaderMap| async move {
                let path = uri.path();
                // `nest` leaves `/api/` (an empty tail) to this fallback; it is still an API miss.
                if path == "/api" || path.starts_with("/api/") {
                    return problem::no_route(path);
                }
                ui::respond(assets, &method, path, &headers)
            },
        )
        // Innermost first: panics become 500s that still get security headers and a trace.
        .layer(CatchPanicLayer::new())
        .layer(middleware::map_response(security::headers))
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{self, Body, Bytes},
        http::{Request, StatusCode},
        response::Response,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt as _;

    use super::router;
    use crate::{build_info, tls::Tls, ui::EMBEDDED};

    async fn send(method: &str, uri: &str, headers: &[(&str, &str)]) -> Response {
        let mut request = Request::builder().method(method).uri(uri);
        for &(name, value) in headers {
            request = request.header(name, value);
        }
        router(&EMBEDDED, Tls::for_tests())
            .oneshot(request.body(Body::empty()).unwrap())
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
    async fn health_reports_the_build_identity() {
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
    async fn the_ca_and_what_the_certificate_covers_are_public() {
        let res = send("GET", "/api/v1/tls", &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(header(&res, "cache-control"), Some("no-store"));
        let json: Value = serde_json::from_slice(&body(res).await).unwrap();
        assert!(
            json["ca_name"]
                .as_str()
                .unwrap()
                .starts_with("uniwrt local CA ")
        );
        assert_eq!(json["ca_sha256"].as_str().unwrap().len(), 95);
        assert!(
            json["names"]
                .as_array()
                .unwrap()
                .contains(&json!("uniwrt.lan"))
        );

        let res = send("GET", "/api/v1/tls/ca.crt", &[]).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            header(&res, "content-type"),
            Some("application/x-x509-ca-cert")
        );
        assert_eq!(
            header(&res, "content-disposition"),
            Some("attachment; filename=\"uniwrt-ca.crt\"")
        );
        let pem = body(res).await;
        assert!(pem.starts_with(b"-----BEGIN CERTIFICATE-----"));
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
    async fn wrong_method_is_405_problem_json() {
        let res = send("POST", "/api/v1/health", &[]).await;
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            header(&res, "content-type"),
            Some("application/problem+json")
        );
        assert!(header(&res, "allow").is_some_and(|allow| allow.contains("GET")));
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
