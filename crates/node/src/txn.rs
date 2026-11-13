//! Transactions on a node (features/lifecycle.md, Render → diff → apply; D-012, D-069): the
//! commands of the transaction tool, `node-agent/txn/txn`, which managed nodes keep as
//! [`TOOL`], and the [`Footprint`] that enabling management installs (the tool and
//! `/etc/init.d/uniwrt-guard`).
//!
//! Each function builds a [`Command`]: run its [`request`](Command::request) on the node (over
//! [`Session::exec`](crate::Session::exec) or any login), then [`parse`](Command::parse) the
//! output. A transaction is staged ([`stage`], [`put`]), [`check`]ed, [`apply`]'d with a
//! dead-man timer, and ends with [`confirm`] or [`rollback`], or rolls back by itself when the
//! timer expires or the node boots. [`current`] reads what the node holds before planning.

use std::{
    fmt::{self, Write as _},
    marker::PhantomData,
    sync::LazyLock,
    time::Duration,
};

use bytes::Bytes;
use serde::{Deserialize, de::DeserializeOwned};

use crate::{
    Error, ExecOutput, ExecRequest, Result,
    bundle::{sha256_hex, tar},
};

/// Where a managed node keeps the tool.
pub const TOOL: &str = "/etc/uniwrt/txn";

/// The dead-man timer's bounds, which the tool enforces too.
pub const MIN_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_TIMEOUT: Duration = Duration::from_secs(3600);

/// How long a command may run: `apply` snapshots, installs and checks the firewall.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The tool's error codes ([`Error::Txn`]).
pub mod code {
    pub const BAD_REQUEST: &str = "bad_request";
    /// Nothing staged or open by that id.
    pub const NOT_FOUND: &str = "not_found";
    /// Another transaction is open, or this one is still reloading.
    pub const BUSY: &str = "busy";
    /// `uci` or `fw4 check` rejected the staged config, or a service is missing.
    pub const INVALID: &str = "invalid";
    /// The transaction ended already: its message holds how.
    pub const EXPIRED: &str = "expired";
    pub const TOO_LARGE: &str = "too_large";
    /// The node lacks what the tool needs (`flock`, procd, the guard).
    pub const UNSUPPORTED: &str = "unsupported";
    pub const FAILED: &str = "failed";
}

/// A transaction's id: 1 to 64 of `A-Z a-z 0-9 _ -`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TxnId(String);

impl TxnId {
    pub fn new(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if valid_name(&id) {
            Ok(Self(id))
        } else {
            Err(Error::Invalid {
                what: format!("transaction id {id:?}"),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TxnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A network device name, as the kernel allows it (`lan1`, `eth0.2`).
fn valid_device(name: &str) -> bool {
    (1..=15).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}

/// Names the tool takes: ids, packages and services.
fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A command of the tool, and how to read its answer.
#[derive(Debug, Clone)]
pub struct Command<T> {
    pub request: ExecRequest,
    name: &'static str,
    output: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Command<T> {
    fn new(name: &'static str, argv: Vec<String>) -> Self {
        Self {
            request: ExecRequest::new(argv).timeout(TIMEOUT),
            name,
            output: PhantomData,
        }
    }

    fn tool(name: &'static str, args: &[&str]) -> Self {
        let mut command = vec![TOOL.to_owned(), name.to_owned()];
        command.extend(args.iter().map(|&arg| arg.to_owned()));
        Self::new(name, command)
    }

    /// The answer, from the last line of stdout: `{"ok":…}`, or `{"err":…}` as
    /// [`Error::Txn`]. A command that printed neither failed otherwise ([`Error::Failed`]).
    pub fn parse(&self, output: ExecOutput) -> Result<T> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Reply<T> {
            Ok(T),
            Err { code: String, msg: String },
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.lines().rev().find(|line| !line.trim().is_empty());
        match line.map(serde_json::from_str::<Reply<T>>) {
            Some(Ok(Reply::Ok(value))) => Ok(value),
            Some(Ok(Reply::Err { code, msg })) => Err(Error::Txn {
                command: self.name.to_owned(),
                code,
                msg,
            }),
            reply => {
                let what = format!("txn {}", self.name);
                let output = output.check(&what)?;
                let err = reply.and_then(Result::err).map_or_else(
                    || "no answer".to_owned(),
                    |err| format!("unexpected answer: {err}"),
                );
                Err(Error::Protocol(format!(
                    "{what}: {err} in {:?}",
                    String::from_utf8_lossy(&output.stdout)
                )))
            }
        }
    }
}

/// An answer without content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Done {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Put {
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Checked {
    pub packages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Armed {
    pub timeout_s: u64,
}

/// What `bounce` answers: how many ports go down and up again.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Bounced {
    pub ports: u64,
}

/// What the tool reports: its footprint's version, the open transaction and the last one that
/// ended.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Status {
    pub version: String,
    pub open: Option<Open>,
    pub last: Option<Finished>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Open {
    pub id: String,
    pub state: OpenState,
    /// Until the timer rolls it back.
    pub remaining_s: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenState {
    /// Installing its packages; the timer is armed.
    Applying,
    /// Installed; procd is reloading what changed.
    Reloading,
    /// Reloaded: ready to be verified and confirmed.
    Applied,
    /// Restoring its snapshot: the timer expired or the controller asked.
    RollingBack,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Finished {
    pub id: String,
    pub outcome: Outcome,
    pub reason: Option<Reason>,
    /// The node's clock, in seconds since the epoch.
    pub at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Confirmed,
    RolledBack,
}

/// Why a transaction rolled back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Not confirmed before the deadline.
    Timeout,
    /// The node booted with it open.
    Boot,
    /// The controller asked.
    Requested,
    /// It could not be installed, or `fw4 check` rejected it.
    Invalid,
    #[serde(other)]
    Unknown,
}

/// Starts staging transaction `id`, which switches `services` on (`true`) or off, replacing
/// any other stage. Fails while a transaction is open.
pub fn stage<'a>(
    id: &TxnId,
    services: impl IntoIterator<Item = (&'a str, bool)>,
) -> Result<Command<Done>> {
    let mut args = vec![id.as_str().to_owned()];
    for (service, on) in services {
        if !valid_name(service) {
            return Err(Error::Invalid {
                what: format!("service name {service:?}"),
            });
        }
        args.push(format!("{service}={}", if on { "on" } else { "off" }));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(Command::tool("stage", &args))
}

/// Stages `package`'s new file.
pub fn put(id: &TxnId, package: &str, content: impl Into<Bytes>) -> Result<Command<Put>> {
    if !valid_name(package) {
        return Err(Error::Invalid {
            what: format!("package name {package:?}"),
        });
    }
    let mut command = Command::tool("put", &[id.as_str(), package]);
    command.request = command.request.stdin(content);
    Ok(command)
}

/// Parses the staged packages with `uci`, and checks that the services to start exist.
pub fn check(id: &TxnId) -> Command<Checked> {
    Command::tool("check", &[id.as_str()])
}

/// Snapshots what the transaction replaces, arms the timer for `timeout`, installs the
/// packages, switches the services, checks the firewall (`fw4 check`), and has procd reload
/// what changed. It answers before the reload, which may cut the connection.
pub fn apply(id: &TxnId, timeout: Duration) -> Command<Armed> {
    Command::tool("apply", &[id.as_str(), &timeout.as_secs().to_string()])
}

/// [`apply`] where the `radios` (UCI `wifi-device` sections) move live by a channel switch
/// (D-113): nothing reloads what it installs, and a rollback restarts the radios on what they
/// held. The transaction installs only what the switch did (their `channel` and `htmode`).
pub fn apply_live<'a>(
    id: &TxnId,
    timeout: Duration,
    radios: impl IntoIterator<Item = &'a str>,
) -> Result<Command<Armed>> {
    let mut args = vec![id.as_str().to_owned(), timeout.as_secs().to_string()];
    for radio in radios {
        if !valid_name(radio) {
            return Err(Error::Invalid {
                what: format!("radio name {radio:?}"),
            });
        }
        args.push(radio.to_owned());
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(Command::tool("apply", &args))
}

/// Moves the open transaction's deadline to `timeout` from now.
pub fn extend(id: &TxnId, timeout: Duration) -> Command<Armed> {
    Command::tool("extend", &[id.as_str(), &timeout.as_secs().to_string()])
}

/// Once the open transaction is applied, takes `ports` down for `down` (1–30 s) and up again,
/// detached from the login, so the hosts behind them ask for a lease again (make-before-break
/// renumbering, D-081); a rollback bounces them once more. Fails with [`code::BUSY`] while
/// the transaction still reloads.
pub fn bounce<'a>(
    ports: impl IntoIterator<Item = &'a str>,
    down: Duration,
) -> Result<Command<Bounced>> {
    let secs = down.as_secs().clamp(1, 30).to_string();
    let mut args = vec![secs];
    for port in ports {
        if !valid_device(port) {
            return Err(Error::Invalid {
                what: format!("port name {port:?}"),
            });
        }
        args.push(port.to_owned());
    }
    if args.len() == 1 {
        return Err(Error::Invalid {
            what: "no ports to bounce".to_owned(),
        });
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(Command::tool("bounce", &args))
}

/// Keeps the applied transaction; fails with [`code::BUSY`] while it reloads, and with
/// [`code::EXPIRED`] once it rolled back.
pub fn confirm(id: &TxnId) -> Command<Done> {
    Command::tool("confirm", &[id.as_str()])
}

/// Restores the snapshot of the open transaction and reloads.
pub fn rollback(id: &TxnId) -> Command<Done> {
    Command::tool("rollback", &[id.as_str()])
}

/// The tool's status; `None` when the node has no footprint.
pub fn status() -> Command<Option<Status>> {
    let script =
        format!(r#"[ -x {TOOL} ] || {{ echo '{{"ok":null}}'; exit 0; }}; exec {TOOL} status"#);
    Command::new("status", vec!["sh".to_owned(), "-c".to_owned(), script])
}

/// Removes the footprint (management off); fails while a transaction is open. A node without
/// one answers as if it had removed it.
pub fn uninstall() -> Command<Done> {
    let script =
        format!(r#"[ -x {TOOL} ] || {{ echo '{{"ok":{{}}}}'; exit 0; }}; exec {TOOL} uninstall"#);
    Command::new("uninstall", vec!["sh".to_owned(), "-c".to_owned(), script])
}

/// A file of `node-agent/txn/`, by its name there.
macro_rules! file {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!("../../../node-agent/txn/", $name)),
        )
    };
}

/// The footprint's files; a test keeps the list in step with the directory.
const FILES: [(&str, &str); 2] = [file!("txn"), file!("uniwrt-guard")];

/// Extracts the footprint from stdin into `/tmp/uniwrt/footprint`, checks every file against
/// SHA256SUMS, and has the new tool install itself and the guard. A constant: nothing is
/// interpolated.
const INSTALL: &str = "set -e; umask 077; mkdir -p /tmp/uniwrt; chown 0:0 /tmp/uniwrt; \
    chmod 700 /tmp/uniwrt; cd /tmp/uniwrt; rm -rf footprint; mkdir footprint; cd footprint; \
    tar xf -; sha256sum -c -s SHA256SUMS; set +e; sh ./txn install \"$PWD\"; status=$?; \
    cd ..; rm -rf footprint; exit $status";

static FOOTPRINT: LazyLock<Footprint> = LazyLock::new(Footprint::build);

/// What a managed node keeps (ARCHITECTURE.md, Node footprint): the tool and the guard, by
/// version, the SHA-256 of their SHA256SUMS.
#[derive(Debug)]
pub struct Footprint {
    version: String,
    tar: Bytes,
}

impl Footprint {
    pub fn get() -> &'static Self {
        &FOOTPRINT
    }

    /// What [`Status::version`] reports once it is installed.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Installs it, replacing an older one; fails while a transaction is open.
    pub fn install(&self) -> Command<Done> {
        let mut command = Command::new(
            "install",
            vec!["sh".to_owned(), "-c".to_owned(), INSTALL.to_owned()],
        );
        command.request = command.request.stdin(self.tar.clone());
        command
    }

    fn build() -> Self {
        let mut sums = String::new();
        for (name, content) in FILES {
            let _ = writeln!(sums, "{}  {name}", sha256_hex(content.as_bytes()));
        }
        let version = sha256_hex(sums.as_bytes());
        let mut archive = tar::Writer::default();
        for (name, content) in FILES {
            archive.file(name, content.as_bytes());
        }
        archive.file("SHA256SUMS", sums.as_bytes());
        archive.file("VERSION", format!("{version}\n").as_bytes());
        Self {
            version,
            tar: archive.finish().into(),
        }
    }
}

pub mod current;
#[cfg(test)]
mod tests;
