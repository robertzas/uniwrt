//! SSH sessions with nodes: connecting directly or through a jump session (`direct-tcpip`),
//! checking the host key against its pin, authenticating, and running commands.

mod channel;
mod client;

use std::{
    fmt,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use bytes::Bytes;
use russh::{
    Disconnect,
    client::{self as russh_client, AuthResult, Handle},
};
use tokio::{net::TcpStream, sync::watch};
use zeroize::Zeroizing;

use self::client::Client;
use crate::{
    ControllerKey, Error, HostKey, Result,
    exec::{self, ExecOutput, ExecRequest, ExecStream},
    shell,
};

/// Where a node's SSH server listens, and as whom to log in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// An IP address or a hostname. Through a jump session, it is resolved by the jump host.
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl Target {
    /// `root` at `host:port`, as on every OpenWrt node.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            user: "root".to_owned(),
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "{}@[{}]:{}", self.user, self.host, self.port)
        } else {
            write!(f, "{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// How to log in: the controller key, or a password (only to install the key at adoption).
#[derive(Clone)]
pub enum Credential {
    Key(ControllerKey),
    Password(Zeroizing<String>),
}

impl Credential {
    fn method(&self) -> &'static str {
        match self {
            Self::Key(_) => "publickey",
            Self::Password(_) => "password",
        }
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => write!(f, "Key({})", key.fingerprint()),
            Self::Password(_) => f.write_str("Password(…)"),
        }
    }
}

/// Connection settings (ARCHITECTURE.md, Node protocol).
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// For TCP, the handshake and authentication together.
    pub connect_timeout: Duration,
    /// Silence after which the client asks the node whether it is still there.
    pub keepalive_interval: Duration,
    /// Unanswered keepalives after which the session counts as dead.
    pub keepalive_max: usize,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            keepalive_interval: Duration::from_secs(15),
            keepalive_max: 3,
        }
    }
}

impl SshConfig {
    fn russh(&self) -> Arc<russh_client::Config> {
        Arc::new(russh_client::Config {
            keepalive_interval: Some(self.keepalive_interval),
            keepalive_max: self.keepalive_max,
            nodelay: true,
            ..russh_client::Config::default()
        })
    }
}

/// Reads a node's host key without logging in: the first step of trust on first use. The user
/// confirms the fingerprint, and the caller pins the key.
pub async fn probe(target: &Target, via: Option<&Session>, config: &SshConfig) -> Result<HostKey> {
    with_timeout(target, config, async {
        // Nothing is trusted while probing, so the handshake stops after the key check.
        match handshake(target, via, None, config).await? {
            Handshake::Untrusted(host_key) | Handshake::Trusted { host_key, .. } => Ok(host_key),
        }
    })
    .await
}

/// One authenticated SSH connection to a node; clones share it. Channels are multiplexed: any
/// number of commands may run at once. The connection closes when the last clone and the last
/// stream drop, or on [`close`](Self::close).
#[derive(Clone)]
pub struct Session(Arc<Inner>);

struct Inner {
    handle: Handle<Client>,
    target: Target,
    name: String,
    host_key: HostKey,
    closed: watch::Receiver<Option<String>>,
}

impl Session {
    /// Connects, checks that the node presents the `pinned` host key, and logs in. Through
    /// `via`, the connection is tunnelled over a `direct-tcpip` channel of that session.
    pub async fn connect(
        target: &Target,
        via: Option<&Session>,
        pinned: &HostKey,
        credential: &Credential,
        config: &SshConfig,
    ) -> Result<Self> {
        let name = match via {
            Some(via) => format!("{target} via {}", via.0.name),
            None => target.to_string(),
        };
        with_timeout(target, config, async {
            let (mut handle, host_key, closed) =
                match handshake(target, via, Some(pinned), config).await? {
                    Handshake::Trusted {
                        handle,
                        host_key,
                        closed,
                    } => (handle, host_key, closed),
                    Handshake::Untrusted(presented) => {
                        return Err(Error::HostKeyMismatch {
                            target: name.clone(),
                            presented,
                            pinned: pinned.clone(),
                        });
                    }
                };
            let user = target.user.as_str();
            let auth = match credential {
                Credential::Key(key) => handle.authenticate_publickey(user, key.credential()).await,
                Credential::Password(password) => {
                    handle.authenticate_password(user, password.as_str()).await
                }
            }
            .map_err(|err| Error::ssh(&name, err))?;
            if !matches!(auth, AuthResult::Success) {
                return Err(Error::AuthRejected {
                    target: name.clone(),
                    user: target.user.clone(),
                    method: credential.method(),
                });
            }
            Ok(Self(Arc::new(Inner {
                handle,
                target: target.clone(),
                name: name.clone(),
                host_key,
                closed,
            })))
        })
        .await
    }

    pub fn target(&self) -> &Target {
        &self.0.target
    }

    /// The (pinned) host key the node presented.
    pub fn host_key(&self) -> &HostKey {
        &self.0.host_key
    }

    /// Runs a command to completion; see [`ExecRequest`] for stdin, the timeout and the limit.
    /// A timeout closes the channel, but a remote process that ignores it may linger.
    pub async fn exec(&self, request: &ExecRequest) -> Result<ExecOutput> {
        exec::collect(request, self.stream(&request.argv)).await
    }

    /// Starts a long-running command, such as the agent or a log follower.
    pub async fn stream<S: AsRef<str>>(&self, argv: &[S]) -> Result<ExecStream> {
        let command = shell::command_line(argv)?;
        let what = format!(
            "`{}` on {}",
            argv.first().map_or("", AsRef::as_ref),
            self.0.name
        );
        channel::open(&self.0.handle, &self.0.name, command, what).await
    }

    /// Writes `data` to the absolute `path` with permission bits `mode`. It goes to a temporary
    /// file next to `path` first and is renamed over it, so readers never see half a file.
    pub async fn upload(&self, path: &str, data: Bytes, mode: u32) -> Result<()> {
        let request = upload_request(path, data, mode)?;
        self.exec(&request).await?.check(&request.what())?;
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.0.handle.is_closed()
    }

    /// Waits until the connection has ended, and says why.
    pub async fn closed(&self) -> String {
        let mut closed = self.0.closed.clone();
        let _ = closed.wait_for(Option::is_some).await;
        closed
            .borrow()
            .clone()
            .unwrap_or_else(|| "the connection closed".to_owned())
    }

    /// Disconnects. Commands still running are cut off.
    pub async fn close(&self) {
        let _ = self
            .0
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Session({})", self.0.name)
    }
}

/// Moves stdin into place with POSIX `sh`; the path and mode are arguments, never syntax.
const UPLOAD: &str = r#"umask 077 && cat >"$1.uniwrt-tmp" && chmod "$2" "$1.uniwrt-tmp" && mv -f "$1.uniwrt-tmp" "$1""#;

pub(crate) fn upload_request(path: &str, data: Bytes, mode: u32) -> Result<ExecRequest> {
    if !path.starts_with('/') {
        return Err(Error::InvalidArgv("an upload path must be absolute"));
    }
    if mode > 0o7777 {
        return Err(Error::InvalidArgv("a file mode has at most 12 bits"));
    }
    // Slow flash writes at a few hundred KB/s, on top of the usual allowance.
    let transfer = Duration::from_secs(u64::try_from(data.len() >> 18).unwrap_or(u64::MAX));
    Ok(
        ExecRequest::new(["sh", "-c", UPLOAD, "sh", path, &format!("{mode:o}")])
            .stdin(data)
            .timeout(exec::DEFAULT_TIMEOUT + transfer),
    )
}

enum Handshake {
    Trusted {
        handle: Handle<Client>,
        host_key: HostKey,
        closed: watch::Receiver<Option<String>>,
    },
    Untrusted(HostKey),
}

async fn handshake(
    target: &Target,
    via: Option<&Session>,
    pinned: Option<&HostKey>,
    config: &SshConfig,
) -> Result<Handshake> {
    let presented = Arc::new(Mutex::new(None));
    let (closed_tx, closed) = watch::channel(None);
    let client = Client {
        pinned: pinned.cloned(),
        presented: Arc::clone(&presented),
        closed: closed_tx,
    };
    let result = match via {
        None => {
            let stream = TcpStream::connect((target.host.as_str(), target.port))
                .await
                .map_err(|source| Error::Connect {
                    target: target.to_string(),
                    source,
                })?;
            russh_client::connect_stream(config.russh(), stream, client).await
        }
        Some(via) => {
            let name = format!("{target} via {}", via.0.name);
            let tunnel = via
                .0
                .handle
                .channel_open_direct_tcpip(
                    target.host.clone(),
                    u32::from(target.port),
                    "127.0.0.1",
                    0,
                )
                .await
                .map_err(|err| Error::ssh(&name, err))?;
            russh_client::connect_stream(config.russh(), tunnel.into_stream(), client).await
        }
    };
    let presented = presented
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    match (result, presented) {
        (Ok(handle), Some(host_key)) => Ok(Handshake::Trusted {
            handle,
            host_key,
            closed,
        }),
        (Err(russh::Error::UnknownKey), Some(presented)) => Ok(Handshake::Untrusted(presented)),
        (Ok(_), None) => Err(Error::Protocol(
            "the session started without a host key check".to_owned(),
        )),
        (Err(err), _) => Err(Error::ssh(target, err)),
    }
}

async fn with_timeout<T>(
    target: &Target,
    config: &SshConfig,
    connect: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(config.connect_timeout, connect)
        .await
        .map_err(|_| Error::Timeout {
            what: format!("connecting to {target}"),
            after: config.connect_timeout,
        })?
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{Target, upload_request};

    #[test]
    fn targets_display_like_ssh_destinations() {
        assert_eq!(Target::new("10.0.0.2", 22).to_string(), "root@10.0.0.2:22");
        assert_eq!(
            Target::new("fd00::1", 2222).to_string(),
            "root@[fd00::1]:2222"
        );
    }

    #[test]
    fn uploads_pass_the_path_and_mode_as_arguments() {
        let request = upload_request("/tmp/it's here", Bytes::from_static(b"x"), 0o640).unwrap();
        assert_eq!(&request.argv[3..], ["sh", "/tmp/it's here", "640"]);
        assert!(upload_request("relative", Bytes::new(), 0o600).is_err());
        assert!(upload_request("/tmp/x", Bytes::new(), 0o10000).is_err());
    }
}
