//! [`banner`]: the version line an SSH server sends as soon as a client connects (RFC 4253,
//! 4.2), read without starting a handshake, so that discovery can tell dropbear, and so likely
//! OpenWrt, from other hosts with one cheap connection.

use std::time::Duration;

use tokio::{
    io::AsyncReadExt as _,
    net::TcpStream,
    time::{Instant, timeout_at},
};

use crate::{Error, Result, Target};

/// A server may send other lines before its version line; one that sends more than this is not
/// worth reading.
const MAX_READ: usize = 1024;
/// The version line's limit (RFC 4253, 4.2).
const MAX_LINE: usize = 255;

/// The version line of the server at `target`, such as `SSH-2.0-dropbear_2024.86`, within
/// `timeout`; `None` when what listens there is not an SSH server.
pub async fn banner(target: &Target, timeout: Duration) -> Result<Option<String>> {
    let deadline = Instant::now() + timeout;
    let timed_out = || Error::Timeout {
        what: format!("reading the SSH banner of {target}"),
        after: timeout,
    };
    let mut stream = timeout_at(
        deadline,
        TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| timed_out())?
    .map_err(|source| Error::Connect {
        target: target.to_string(),
        source,
    })?;
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0; 256];
    loop {
        let n = match timeout_at(deadline, stream.read(&mut chunk)).await {
            // A read error after connecting is a server that hung up: not SSH.
            Ok(read) => read.unwrap_or(0),
            // A server that spoke whole lines, none of them SSH's, is something else.
            Err(_) if buf.contains(&b'\n') => return Ok(None),
            Err(_) => return Err(timed_out()),
        };
        buf.extend_from_slice(&chunk[..n]);
        match version_line(&buf) {
            Line::Version(line) => return Ok(Some(line)),
            Line::Overlong => return Ok(None),
            Line::Pending if n == 0 || buf.len() >= MAX_READ => return Ok(None),
            Line::Pending => {}
        }
    }
}

/// What the server sent so far.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// The version line.
    Version(String),
    /// A line too long for SSH: not an SSH server.
    Overlong,
    /// Only other lines, or the start of one.
    Pending,
}

fn version_line(buf: &[u8]) -> Line {
    let mut lines = buf.split(|&b| b == b'\n').peekable();
    while let Some(line) = lines.next() {
        if line.len() > MAX_LINE + 1 {
            return Line::Overlong;
        }
        // The last piece has no newline yet.
        if lines.peek().is_none() {
            break;
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"SSH-") {
            return Line::Version(String::from_utf8_lossy(line).into_owned());
        }
    }
    Line::Pending
}

#[cfg(test)]
mod tests {
    use tokio::{io::AsyncWriteExt as _, net::TcpListener};

    use super::*;

    #[test]
    fn the_version_line_may_follow_other_lines() {
        assert_eq!(version_line(b"SSH-2.0-dropb"), Line::Pending);
        assert_eq!(
            version_line(b"SSH-2.0-dropbear_2024.86\r\n"),
            Line::Version("SSH-2.0-dropbear_2024.86".to_owned())
        );
        assert_eq!(
            version_line(b"Welcome\r\nSSH-2.0-OpenSSH_9.9\n"),
            Line::Version("SSH-2.0-OpenSSH_9.9".to_owned())
        );
        assert_eq!(version_line(b"Welcome\r\n"), Line::Pending);
        assert_eq!(version_line(&[b'x'; 300]), Line::Overlong);
        let long = [b"SSH-2.0-".as_slice(), &[b'x'; 300], b"\n"].concat();
        assert_eq!(version_line(&long), Line::Overlong);
    }

    async fn serve(reply: &'static [u8]) -> Target {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.write_all(reply).await.unwrap();
            // Held open: the reader must not wait for the end of the stream.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        Target::new("127.0.0.1", port)
    }

    #[tokio::test]
    async fn reads_the_banner_and_tells_other_servers_apart() {
        let timeout = Duration::from_secs(5);
        let dropbear = serve(b"SSH-2.0-dropbear_2024.86\r\n").await;
        assert_eq!(
            banner(&dropbear, timeout).await.unwrap().as_deref(),
            Some("SSH-2.0-dropbear_2024.86")
        );
        let http = serve(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
        assert_eq!(
            banner(&http, Duration::from_millis(100)).await.unwrap(),
            None
        );
        let silent = serve(b"").await;
        assert!(matches!(
            banner(&silent, Duration::from_millis(100)).await,
            Err(Error::Timeout { .. })
        ));
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        assert!(matches!(
            banner(&Target::new("127.0.0.1", port), timeout).await,
            Err(Error::Connect { .. })
        ));
    }
}
