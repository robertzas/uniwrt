use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

use super::{Fetched, HttpSource, Source as _, Validators};
use crate::Error;

/// A loopback server: `/moved` redirects to `/list`; `/list` answers 304 to its own `ETag`,
/// else the list; anything else 404. Each connection takes one request.
async fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let location = format!("{base}/list");
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let location = location.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") {
                    let Ok(n) = socket.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                let answer = if request.starts_with("get /moved ") {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                    )
                } else if request.starts_with("get /list ")
                    && request.contains("if-none-match: \"v1\"")
                {
                    "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n".to_owned()
                } else if request.starts_with("get /list ") {
                    let body = "0.0.0.0 ads.example.com\n";
                    format!(
                        "HTTP/1.1 200 OK\r\nETag: \"v1\"\r\nLast-Modified: Tue, 17 Nov 2026 03:00:00 GMT\r\n\
                         Content-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned()
                };
                let _ = socket.write_all(answer.as_bytes()).await;
            });
        }
    });
    base
}

#[tokio::test]
async fn gets_follow_redirects_and_ask_conditionally() {
    let base = serve().await;
    let source = HttpSource::new("uniwrt-test").unwrap();
    let fresh = Validators::default();
    let Fetched::Body { body, validators } = source
        .fetch(&format!("{base}/moved"), &fresh)
        .await
        .unwrap()
    else {
        panic!("no body");
    };
    assert_eq!(&body[..], b"0.0.0.0 ads.example.com\n");
    assert_eq!(validators.etag.as_deref(), Some("\"v1\""));
    assert!(validators.last_modified.is_some());
    let again = source
        .fetch(&format!("{base}/list"), &validators)
        .await
        .unwrap();
    assert_eq!(again, Fetched::NotModified);
    let missing = source.fetch(&format!("{base}/gone"), &fresh).await;
    assert!(
        matches!(missing, Err(Error::Status { status: 404, .. })),
        "{missing:?}"
    );
}
