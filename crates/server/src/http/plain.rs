//! The plain HTTP listener (features/security.md, TLS; D-039): it redirects to HTTPS, except
//! for the health check (Docker probes it) and for requests from a trusted reverse proxy, which
//! terminates TLS itself and gets the whole application.

use std::{net::IpAddr, sync::Arc};

use axum::{
    Router,
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode, header, uri::Authority},
    middleware,
    response::{IntoResponse as _, Response},
};
use ipnet::IpNet;
use tower::ServiceExt as _;
use tower_http::trace::TraceLayer;

use super::{problem, security};
use crate::tls::listener::Peer;

const HEALTH: &str = "/api/v1/health";

#[derive(Clone)]
struct Plain {
    app: Router,
    redirect: Router,
    trusted: Arc<[IpNet]>,
}

/// The HTTP listener's router: `app` for the health check and trusted proxies, else a redirect
/// to the same host on `https_port`. Serve it with `ConnectInfo<Peer>`.
pub(crate) fn router(app: Router, https_port: u16, trusted: Vec<IpNet>) -> Router {
    let redirect = Router::new()
        .fallback(move |request: Request| async move { redirect(&request, https_port) })
        .layer(middleware::map_response(security::headers))
        .layer(TraceLayer::new_for_http());
    let plain = Plain {
        app,
        redirect,
        trusted: trusted.into(),
    };
    Router::new().fallback(serve).with_state(plain)
}

async fn serve(
    State(plain): State<Plain>,
    ConnectInfo(Peer(peer)): ConnectInfo<Peer>,
    request: Request,
) -> Response {
    let target = if request.uri().path() == HEALTH || is_trusted(&plain.trusted, peer.ip()) {
        plain.app
    } else {
        plain.redirect
    };
    match target.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

/// A dual-stack socket reports IPv4 clients as IPv4-mapped IPv6 addresses.
fn is_trusted(trusted: &[IpNet], ip: IpAddr) -> bool {
    let ip = ip.to_canonical();
    trusted.iter().any(|net| net.contains(&ip))
}

/// 307 keeps the method and body, and unlike 308 is not cached: trusting a proxy later must
/// not be undone by browsers that remember the redirect.
fn redirect(request: &Request, https_port: u16) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| host.parse::<Authority>().ok())
        .or_else(|| request.uri().authority().cloned());
    let Some(host) = host else {
        return problem::response(
            StatusCode::BAD_REQUEST,
            "Plain HTTP only redirects to HTTPS, which needs the Host header".to_owned(),
        );
    };
    let port = if https_port == 443 {
        String::new()
    } else {
        format!(":{https_port}")
    };
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", |path| path.as_str());
    let location = format!("https://{}{port}{path}", host.host());
    match HeaderValue::try_from(location) {
        Ok(location) => (
            StatusCode::TEMPORARY_REDIRECT,
            [
                (header::LOCATION, location),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
        )
            .into_response(),
        Err(_) => problem::response(
            StatusCode::BAD_REQUEST,
            "Malformed request target".to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{
        Router,
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        response::Response,
        routing::get,
    };
    use tower::ServiceExt as _;

    use super::router;
    use crate::tls::listener::Peer;

    fn app() -> Router {
        Router::new()
            .route("/api/v1/health", get(|| async { "healthy" }))
            .fallback(|| async { "the app" })
    }

    async fn send(peer: &str, uri: &str, host: Option<&str>) -> Response {
        let trusted = vec!["127.0.0.1/32".parse().unwrap(), "fd00::/8".parse().unwrap()];
        let mut request = Request::builder().uri(uri);
        if let Some(host) = host {
            request = request.header("host", host);
        }
        let mut request = request.body(Body::empty()).unwrap();
        let peer: SocketAddr = peer.parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(Peer(peer)));
        router(app(), 8443, trusted).oneshot(request).await.unwrap()
    }

    fn location(response: &Response) -> &str {
        response.headers()["location"].to_str().unwrap()
    }

    async fn text(response: Response) -> String {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn others_are_redirected_to_the_same_host_over_https() {
        let res = send(
            "192.168.1.20:5000",
            "/nodes/7?tab=wifi",
            Some("192.168.1.10:8080"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(location(&res), "https://192.168.1.10:8443/nodes/7?tab=wifi");
        assert_eq!(res.headers()["cache-control"], "no-store");
        assert_eq!(res.headers()["x-frame-options"], "DENY");

        let res = send("[fd00::9]:5000", "/", Some("[fd00::1]:8080")).await;
        assert_eq!(res.status(), StatusCode::OK, "trusted");
        let res = send("[2001:db8::9]:5000", "/", Some("[2001:db8::1]:8080")).await;
        assert_eq!(location(&res), "https://[2001:db8::1]:8443/");
        let res = send("192.168.1.20:5000", "/", Some("uniwrt.lan")).await;
        assert_eq!(location(&res), "https://uniwrt.lan:8443/");
    }

    #[tokio::test]
    async fn a_request_without_a_host_cannot_be_redirected() {
        let res = send("192.168.1.20:5000", "/", None).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(res.headers()["content-type"], "application/problem+json");
        let res = send("192.168.1.20:5000", "/", Some("bad host")).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn the_health_check_is_served_to_anyone() {
        let res = send("192.168.1.20:5000", "/api/v1/health", Some("x")).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(text(res).await, "healthy");
    }

    #[tokio::test]
    async fn trusted_proxies_get_the_app_also_over_ipv4_mapped_addresses() {
        for peer in ["127.0.0.1:5000", "[::ffff:127.0.0.1]:5000"] {
            let res = send(peer, "/nodes", Some("uniwrt.example.com")).await;
            assert_eq!(res.status(), StatusCode::OK, "{peer}");
            assert_eq!(text(res).await, "the app");
        }
    }
}
