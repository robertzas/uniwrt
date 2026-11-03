//! Delivery over loopback: an HTTP and an HTTPS server (its CA trusted through
//! [`Sender::with_roots`]) that record one request each, and an SMTP server that upgrades with
//! STARTTLS and records what it is given.

use std::net::SocketAddr;

use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use serde_json::Value;
use tokio::{
    io::{
        AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _,
        BufReader,
    },
    net::TcpListener,
    task::JoinHandle,
};
use tokio_rustls::TlsAcceptor;
use uniwrt_model::notify::{EmailSpec, NotifierSpec, SmtpSecurity, WebhookFormat, WebhookSpec};

use crate::{Header, Secrets, SendError, Sender, validate::sample};

/// A CA, and a server configuration for `localhost` that it signed.
fn pki() -> (CertificateDer<'static>, TlsAcceptor) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca.clone().self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);
    let key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .signed_by(&key, &issuer)
        .unwrap();
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        )
        .unwrap();
    (
        ca_cert.der().clone(),
        TlsAcceptor::from(std::sync::Arc::new(config)),
    )
}

/// What a server was sent: the request's head and its body.
struct Seen {
    head: String,
    body: Vec<u8>,
}

/// Answers one HTTP request with `status` and `answer`.
async fn answer<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    status: &str,
    answer: &str,
) -> Seen {
    let mut data = Vec::new();
    let mut chunk = [0; 4096];
    let end = loop {
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "the request ended early");
        data.extend_from_slice(&chunk[..n]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let length: usize = head
        .lines()
        .find_map(|line| {
            line.to_lowercase()
                .strip_prefix("content-length: ")
                .map(str::to_owned)
        })
        .map_or(0, |n| n.trim().parse().unwrap());
    let mut body = data[end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.unwrap();
        body.extend_from_slice(&chunk[..n]);
    }
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
        answer.len()
    );
    stream.write_all(response.as_bytes()).await.unwrap();
    stream.shutdown().await.ok();
    Seen { head, body }
}

async fn http_server(
    tls: Option<TlsAcceptor>,
    status: &'static str,
    body: &'static str,
) -> (SocketAddr, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        match tls {
            Some(acceptor) => answer(acceptor.accept(tcp).await.unwrap(), status, body).await,
            None => answer(tcp, status, body).await,
        }
    });
    (addr, task)
}

fn webhook(url: String) -> (NotifierSpec, Secrets) {
    let spec = NotifierSpec::Webhook(WebhookSpec {
        format: WebhookFormat::Generic,
        template: None,
    });
    let mut secrets = Secrets::default();
    secrets.url = Some(url);
    secrets.headers = vec![Header {
        name: "Authorization".to_owned(),
        value: "Bearer s3cret".to_owned(),
    }];
    (spec, secrets)
}

#[tokio::test]
async fn posts_json_over_http() {
    let (addr, server) = http_server(None, "204 No Content", "").await;
    let (spec, secrets) = webhook(format!("http://{addr}/hook?x=1"));
    let sender = Sender::new("uniwrt/test").unwrap();
    sender.send(&spec, &secrets, &sample()).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen.head.starts_with("POST /hook?x=1 HTTP/1.1\r\n"));
    let head = seen.head.to_lowercase();
    assert!(head.contains("content-type: application/json"));
    assert!(head.contains("authorization: bearer s3cret"));
    assert!(head.contains("user-agent: uniwrt/test"));
    let body: Value = serde_json::from_slice(&seen.body).unwrap();
    assert_eq!(body["title"], "CPU high · Node 1");
}

#[tokio::test]
async fn failures_say_what_and_whether_to_retry() {
    let sender = Sender::new("uniwrt/test").unwrap();
    let (addr, server) = http_server(None, "500 Internal Server Error", "  database\n down ").await;
    let (spec, secrets) = webhook(format!("http://{addr}/"));
    let err = sender.send(&spec, &secrets, &sample()).await.unwrap_err();
    server.await.unwrap();
    assert_eq!(
        err,
        SendError::Status {
            status: 500,
            body: "database down".to_owned()
        }
    );
    assert_eq!(
        err.to_string(),
        "the server answered HTTP 500: database down"
    );
    assert!(err.retryable());

    let (addr, server) = http_server(None, "404 Not Found", "").await;
    let (spec, secrets) = webhook(format!("http://{addr}/"));
    let err = sender.send(&spec, &secrets, &sample()).await.unwrap_err();
    server.await.unwrap();
    assert!(!err.retryable());

    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let (spec, secrets) = webhook(format!("http://{closed}/"));
    let err = sender.send(&spec, &secrets, &sample()).await.unwrap_err();
    assert!(matches!(err, SendError::Connect(_)), "{err}");
    assert!(err.retryable());
}

#[tokio::test]
async fn https_checks_the_certificate() {
    let (ca, acceptor) = pki();
    let (addr, server) = http_server(Some(acceptor.clone()), "200 OK", "ok").await;
    let (spec, secrets) = webhook(format!("https://localhost:{}/hook", addr.port()));
    let trusting = Sender::with_roots("uniwrt/test", vec![ca]).unwrap();
    trusting.send(&spec, &secrets, &sample()).await.unwrap();
    assert!(server.await.unwrap().head.starts_with("POST /hook "));

    let (addr, server) = http_server(Some(acceptor), "200 OK", "ok").await;
    let (spec, secrets) = webhook(format!("https://localhost:{}/hook", addr.port()));
    let err = Sender::new("uniwrt/test")
        .unwrap()
        .send(&spec, &secrets, &sample())
        .await
        .unwrap_err();
    server.abort();
    assert!(
        matches!(&err, SendError::Connect(why) if why.contains("certificate")),
        "{err}"
    );
}

/// An SMTP server for one message: it offers STARTTLS (unless `starttls` is false) and, once
/// upgraded, AUTH; it returns the commands it was given and the message.
async fn smtp_server(acceptor: TlsAcceptor, starttls: bool) -> (u16, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut plain = BufReader::new(tcp);
        plain
            .get_mut()
            .write_all(b"220 localhost ESMTP test\r\n")
            .await
            .unwrap();
        let mut seen = Vec::new();
        loop {
            let mut line = String::new();
            if plain.read_line(&mut line).await.unwrap() == 0 {
                return seen;
            }
            seen.push(line.trim_end().to_owned());
            let reply: &[u8] = match line.get(..4).map(str::to_uppercase).as_deref() {
                Some("EHLO") if starttls => b"250-localhost\r\n250 STARTTLS\r\n",
                Some("EHLO") => b"250-localhost\r\n250 AUTH PLAIN\r\n",
                Some("STAR") => b"220 go ahead\r\n",
                Some("QUIT") => b"221 bye\r\n",
                _ => b"502 no\r\n",
            };
            plain.get_mut().write_all(reply).await.unwrap();
            if line.starts_with("STARTTLS") || line.starts_with("QUIT") {
                break;
            }
        }
        if !starttls {
            return seen;
        }
        let mut tls = BufReader::new(acceptor.accept(plain.into_inner()).await.unwrap());
        loop {
            let mut line = String::new();
            if tls.read_line(&mut line).await.unwrap() == 0 {
                return seen;
            }
            seen.push(line.trim_end().to_owned());
            let command = line.get(..4).map(str::to_uppercase).unwrap_or_default();
            let reply: &[u8] = match command.as_str() {
                "EHLO" => b"250-localhost\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n",
                "AUTH" => b"235 2.7.0 accepted\r\n",
                "DATA" => {
                    tls.get_mut().write_all(b"354 go\r\n").await.unwrap();
                    let mut message = String::new();
                    loop {
                        let mut data = String::new();
                        tls.read_line(&mut data).await.unwrap();
                        if data == ".\r\n" {
                            break;
                        }
                        message.push_str(&data);
                    }
                    seen.push(message);
                    b"250 queued\r\n"
                }
                "QUIT" => {
                    tls.get_mut().write_all(b"221 bye\r\n").await.unwrap();
                    return seen;
                }
                _ => b"250 ok\r\n",
            };
            tls.get_mut().write_all(reply).await.unwrap();
        }
    });
    (port, task)
}

fn email(port: u16) -> (NotifierSpec, Secrets) {
    let spec = NotifierSpec::Email(EmailSpec {
        host: "localhost".to_owned(),
        port,
        security: SmtpSecurity::Starttls,
        username: Some("me".to_owned()),
        from: "uniwrt <uniwrt@example.com>".to_owned(),
        to: vec!["me@example.com".to_owned(), "you@example.com".to_owned()],
    });
    let mut secrets = Secrets::default();
    secrets.password = Some("pw".to_owned());
    (spec, secrets)
}

#[tokio::test]
async fn emails_over_starttls() {
    let (ca, acceptor) = pki();
    let (port, server) = smtp_server(acceptor, true).await;
    let (spec, secrets) = email(port);
    let sender = Sender::with_roots("uniwrt/test", vec![ca]).unwrap();
    sender.send(&spec, &secrets, &sample()).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen.iter().any(|l| l == "STARTTLS"));
    // AUTH PLAIN of "\0me\0pw", only once encrypted.
    let starttls = seen.iter().position(|l| l == "STARTTLS").unwrap();
    let auth = seen
        .iter()
        .position(|l| l == "AUTH PLAIN AG1lAHB3")
        .unwrap();
    assert!(auth > starttls);
    assert!(seen.iter().any(|l| l == "MAIL FROM:<uniwrt@example.com>"));
    assert!(seen.iter().any(|l| l == "RCPT TO:<you@example.com>"));
    let message = seen.iter().find(|l| l.contains("Subject:")).unwrap();
    assert!(message.contains("multipart/alternative"));
    assert!(message.contains("text/html"));
}

#[tokio::test]
async fn never_emails_in_the_clear() {
    let (ca, acceptor) = pki();
    let (port, server) = smtp_server(acceptor, false).await;
    let (spec, secrets) = email(port);
    let sender = Sender::with_roots("uniwrt/test", vec![ca]).unwrap();
    let err = sender.send(&spec, &secrets, &sample()).await.unwrap_err();
    assert!(matches!(err, SendError::Smtp { .. }), "{err}");
    let seen = server.await.unwrap();
    assert!(
        !seen
            .iter()
            .any(|l| l.starts_with("AUTH") || l.starts_with("MAIL"))
    );
}
