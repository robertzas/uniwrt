//! Answers a UI request with an embedded file, the SPA page for a client-side route, or a 404.

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse as _, Response},
};

use super::{
    assets::{Asset, Assets, Variant},
    encoding::{self, Encoding},
};

/// Content-hashed files never change, so browsers keep them for a year without revalidating.
const CACHE_HASHED: &str = "public, max-age=31536000, immutable";
/// Everything else (`index.html`, unhashed files) is revalidated with its `ETag` on every use.
const CACHE_REVALIDATE: &str = "no-cache";

pub(crate) fn respond(
    assets: &Assets,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
) -> Response {
    let head = method == Method::HEAD;
    if method != Method::GET && !head {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    let path = path.trim_start_matches('/');
    let asset = match assets.get(path) {
        Some(asset) => asset,
        None if path.is_empty() || is_client_route(assets, path, headers) => assets.index(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    serve(asset, assets.is_hashed(asset.path), head, headers)
}

/// Mirrors Vite's dev-server fallback, so dev and production route alike: a request that accepts
/// HTML, outside the hashed directory, and not the browser's automatic favicon probe.
fn is_client_route(assets: &Assets, path: &str, headers: &HeaderMap) -> bool {
    if assets.is_hashed(path) || path == "favicon.ico" {
        return false;
    }
    let mut accept = headers.get_all(header::ACCEPT).iter().peekable();
    accept.peek().is_none()
        || accept.any(|value| {
            value
                .to_str()
                .is_ok_and(|v| v.is_empty() || v.contains("text/html") || v.contains("*/*"))
        })
}

fn serve(asset: &Asset, hashed: bool, head: bool, headers: &HeaderMap) -> Response {
    let compressed = asset.brotli.is_some() || asset.gzip.is_some();
    let encoding = encoding::negotiate(
        headers.get_all(header::ACCEPT_ENCODING),
        asset.brotli.is_some(),
        asset.gzip.is_some(),
    );
    // The coding comes from the variant actually chosen, so the two can never disagree.
    let (variant, coding): (&Variant, Option<&'static str>) = match encoding {
        Encoding::Brotli => asset.brotli.as_ref().map(|v| (v, Some("br"))),
        Encoding::Gzip => asset.gzip.as_ref().map(|v| (v, Some("gzip"))),
        Encoding::Identity => None,
    }
    .unwrap_or((&asset.identity, None));

    let fresh = if_none_match(headers, variant.etag);
    let mut response = if fresh || head {
        Response::new(Body::empty())
    } else {
        Response::new(Body::from(variant.bytes))
    };
    let out = response.headers_mut();
    out.insert(header::ETAG, HeaderValue::from_static(variant.etag));
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if hashed {
            CACHE_HASHED
        } else {
            CACHE_REVALIDATE
        }),
    );
    if compressed {
        out.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    if fresh {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    out.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from(variant.bytes.len()),
    );
    if let Some(coding) = coding {
        out.insert(header::CONTENT_ENCODING, HeaderValue::from_static(coding));
    }
    response
}

/// `If-None-Match` (RFC 9110 §13.1.2): weak comparison with the chosen representation's `ETag`.
fn if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{self, Bytes},
        http::{HeaderMap, HeaderValue, Method, StatusCode},
        response::Response,
    };

    use super::respond;
    use crate::ui::assets::{Asset, Assets, Source, Variant};

    const fn v(etag: &'static str, bytes: &'static [u8]) -> Variant {
        Variant { etag, bytes }
    }

    static FILES: [Asset; 3] = [
        Asset {
            path: "assets/app-1a2b3c4d.js",
            content_type: "text/javascript; charset=utf-8",
            identity: v("\"js\"", b"export const answer = 42;"),
            brotli: Some(v("\"js-br\"", b"<br>")),
            gzip: Some(v("\"js-gz\"", b"<gz>")),
        },
        Asset {
            path: "assets/logo-1a2b3c4d.png",
            content_type: "image/png",
            identity: v("\"png\"", b"PNG"),
            brotli: None,
            gzip: None,
        },
        Asset {
            path: "index.html",
            content_type: "text/html; charset=utf-8",
            identity: v("\"html\"", b"<!doctype html>"),
            brotli: Some(v("\"html-br\"", b"<html br>")),
            gzip: None,
        },
    ];
    static ASSETS: Assets = Assets {
        source: Source::WebDist,
        hashed_dir: "assets/",
        files: &FILES,
        index: 2,
    };

    fn request(method: &Method, path: &str, headers: &[(&'static str, &'static str)]) -> Response {
        let mut map = HeaderMap::new();
        for &(name, value) in headers {
            map.append(name, HeaderValue::from_static(value));
        }
        respond(&ASSETS, method, path, &map)
    }

    fn get(path: &str, headers: &[(&'static str, &'static str)]) -> Response {
        request(&Method::GET, path, headers)
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
    async fn serves_the_best_variant_with_matching_headers() {
        let res = get(
            "/assets/app-1a2b3c4d.js",
            &[("accept-encoding", "gzip, deflate, br")],
        );
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(header(&res, "content-encoding"), Some("br"));
        assert_eq!(
            header(&res, "content-type"),
            Some("text/javascript; charset=utf-8")
        );
        assert_eq!(header(&res, "content-length"), Some("4"));
        assert_eq!(header(&res, "etag"), Some("\"js-br\""));
        assert_eq!(header(&res, "vary"), Some("accept-encoding"));
        assert_eq!(
            header(&res, "cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(body(res).await, "<br>");

        let res = get("/assets/app-1a2b3c4d.js", &[("accept-encoding", "gzip")]);
        assert_eq!(header(&res, "content-encoding"), Some("gzip"));
        assert_eq!(header(&res, "etag"), Some("\"js-gz\""));
        assert_eq!(body(res).await, "<gz>");

        let res = get("/assets/app-1a2b3c4d.js", &[]);
        assert_eq!(header(&res, "content-encoding"), None);
        assert_eq!(header(&res, "etag"), Some("\"js\""));
        assert_eq!(header(&res, "vary"), Some("accept-encoding"));
        assert_eq!(body(res).await, "export const answer = 42;");
    }

    #[tokio::test]
    async fn uncompressed_assets_do_not_vary() {
        let res = get(
            "/assets/logo-1a2b3c4d.png",
            &[("accept-encoding", "br"), ("accept", "image/*")],
        );
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(header(&res, "content-encoding"), None);
        assert_eq!(header(&res, "vary"), None);
        assert_eq!(body(res).await, "PNG");
    }

    #[tokio::test]
    async fn index_is_revalidated() {
        for path in ["/", "", "/index.html"] {
            let res = get(path, &[]);
            assert_eq!(
                header(&res, "content-type"),
                Some("text/html; charset=utf-8"),
                "{path:?}"
            );
            assert_eq!(header(&res, "cache-control"), Some("no-cache"));
            assert_eq!(body(res).await, "<!doctype html>");
        }
    }

    #[tokio::test]
    async fn if_none_match_yields_304_for_the_chosen_variant() {
        for tag in ["\"js-br\"", "W/\"js-br\"", "\"other\", \"js-br\"", "*"] {
            let res = get(
                "/assets/app-1a2b3c4d.js",
                &[("accept-encoding", "br"), ("if-none-match", tag)],
            );
            assert_eq!(res.status(), StatusCode::NOT_MODIFIED, "{tag}");
            assert_eq!(header(&res, "etag"), Some("\"js-br\""));
            assert_eq!(header(&res, "vary"), Some("accept-encoding"));
            assert_eq!(header(&res, "content-type"), None);
            assert!(body(res).await.is_empty());
        }
        // The identity tag does not validate the brotli representation.
        let res = get(
            "/assets/app-1a2b3c4d.js",
            &[("accept-encoding", "br"), ("if-none-match", "\"js\"")],
        );
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn client_routes_get_the_spa_page() {
        for accept in [
            &[("accept", "text/html,application/xhtml+xml")][..],
            &[("accept", "*/*")],
            &[],
        ] {
            let res = get("/nodes/10.0.0.1", accept);
            assert_eq!(res.status(), StatusCode::OK, "{accept:?}");
            assert_eq!(header(&res, "cache-control"), Some("no-cache"));
            assert_eq!(body(res).await, "<!doctype html>");
        }
    }

    #[test]
    fn misses_that_are_not_client_routes_are_404() {
        assert_eq!(
            get("/nodes/1", &[("accept", "application/json")]).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get("/assets/stale-00000000.js", &[("accept", "text/html")]).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get("/favicon.ico", &[("accept", "*/*")]).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn head_keeps_headers_and_drops_the_body() {
        let res = request(&Method::HEAD, "/", &[]);
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(header(&res, "content-length"), Some("15"));
        assert!(body(res).await.is_empty());
    }

    #[test]
    fn other_methods_are_rejected() {
        let res = request(&Method::POST, "/", &[]);
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(header(&res, "allow"), Some("GET, HEAD"));
    }
}
