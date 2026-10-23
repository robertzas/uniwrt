//! `uniwrt healthcheck`: `GET /api/v1/health` on the local server; success means a 200.
//!
//! Docker's HEALTHCHECK runs this because scratch images have no curl. It uses a blocking socket
//! with timeouts and starts no async runtime, so each probe stays cheap.

use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    time::Duration,
};

use anyhow::{Context as _, bail};

use crate::cli::HealthcheckArgs;

pub(crate) fn run(args: &HealthcheckArgs) -> anyhow::Result<()> {
    let addr = probe_addr(args.http_listen);
    check(addr, Duration::from_secs(args.timeout))
        .with_context(|| format!("health check of {addr} failed"))
}

/// Where to probe: a server bound to an unspecified IP (0.0.0.0 or ::) answers on loopback.
fn probe_addr(listen: SocketAddr) -> SocketAddr {
    let ip = match listen.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, listen.port())
}

fn check(addr: SocketAddr, timeout: Duration) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write!(
        stream,
        "GET /api/v1/health HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: uniwrt-healthcheck\r\nConnection: close\r\n\r\n"
    )?;
    let mut status_line = String::new();
    BufReader::new(stream.take(1024)).read_line(&mut status_line)?;
    match status_code(&status_line) {
        Some(200) => Ok(()),
        Some(code) => bail!("the server answered {code}"),
        None => bail!("not an HTTP response: {:?}", status_line.trim_end()),
    }
}

/// The code from a status line such as `HTTP/1.1 200 OK`.
fn status_code(line: &str) -> Option<u16> {
    let mut parts = line.split_ascii_whitespace();
    let version = parts.next()?;
    let code = parts.next()?;
    if !version.starts_with("HTTP/1.") || code.len() != 3 {
        return None;
    }
    code.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use axum::Router;
    use tokio::net::TcpListener;

    use super::{check, probe_addr, status_code};
    use crate::{http, ui::EMBEDDED};

    #[test]
    fn unspecified_addresses_are_probed_on_loopback() {
        let probe = |listen: &str| probe_addr(listen.parse().unwrap()).to_string();
        assert_eq!(probe("0.0.0.0:8080"), "127.0.0.1:8080");
        assert_eq!(probe("[::]:8080"), "[::1]:8080");
        assert_eq!(probe("192.168.1.10:9000"), "192.168.1.10:9000");
    }

    #[test]
    fn parses_status_lines() {
        assert_eq!(status_code("HTTP/1.1 200 OK\r\n"), Some(200));
        assert_eq!(
            status_code("HTTP/1.0 503 Service Unavailable\r\n"),
            Some(503)
        );
        for bad in [
            "",
            "SSH-2.0-OpenSSH_9.9\r\n",
            "HTTP/1.1 20 OK",
            "HTTP/1.1 abc",
        ] {
            assert_eq!(status_code(bad), None, "{bad:?}");
        }
    }

    async fn spawn(app: Router) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        addr
    }

    async fn probe(addr: SocketAddr) -> anyhow::Result<()> {
        tokio::task::spawn_blocking(move || check(addr, Duration::from_secs(2)))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn passes_against_a_healthy_server() {
        probe(spawn(http::router(&EMBEDDED)).await).await.unwrap();
    }

    #[tokio::test]
    async fn fails_on_other_statuses_and_closed_ports() {
        let err = probe(spawn(Router::new()).await).await.unwrap_err();
        assert_eq!(err.to_string(), "the server answered 404");

        let closed = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(probe(closed).await.is_err());
    }
}
