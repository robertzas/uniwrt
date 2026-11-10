//! A [`Dial`] over `docker exec`: the controller's logins to the harness's containers. A login
//! goes to the container that holds the target's address now, as SSH would reach it, and must
//! find the host key the container's dropbear presents. Commands run as their argv, never
//! through a shell string; collect runs the working tree's agent (`/tmp/uniwrt/agent`, which
//! scripts/test-system.sh installs). A container can be lost once the transaction tool applied
//! on it: from then on it answers no login and no command, as a node cut off by its change.

use std::{
    collections::BTreeSet,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use tokio::{io::AsyncWriteExt as _, process::Command};
use uniwrt_jobs::{BoxFuture, Dial, Remote};
use uniwrt_node::{
    Credential, Error, ExecOutput, ExecRequest, ExitStatus, HostKey, Result, Target,
    proto::ops::{Collected, Set},
    txn::TOOL,
};

/// How long the agent may take to collect (as `uniwrt_jobs`'s own logins allow).
const COLLECT_TIMEOUT: Duration = Duration::from_secs(30);

struct Container {
    name: String,
    host_key: HostKey,
}

#[derive(Default)]
struct Faults {
    /// Containers that answer nothing.
    lost: BTreeSet<String>,
    /// Lost as soon as the tool's `apply` succeeds on it.
    lose_after_apply: BTreeSet<String>,
    /// The container that stands in for the controller's host: the gateway sees the logins
    /// come from its LAN address.
    controller: Option<String>,
}

struct Inner {
    containers: Vec<Container>,
    faults: Mutex<Faults>,
}

/// One per test; clones share the containers and their faults.
#[derive(Clone)]
pub(crate) struct DockerDial(Arc<Inner>);

impl DockerDial {
    /// Reaches `names`, each with the host key its dropbear presents.
    pub(crate) async fn new(names: &[&str]) -> Self {
        let mut containers = Vec::new();
        for &name in names {
            let request = ExecRequest::new([
                "dropbearkey",
                "-y",
                "-f",
                "/etc/dropbear/dropbear_ed25519_host_key",
            ]);
            let output = exec(name, &request).await.expect("docker exec runs");
            let stdout = String::from_utf8_lossy(&output.stdout);
            let line = stdout
                .lines()
                .find(|line| line.starts_with("ssh-ed25519 "))
                .unwrap_or_else(|| panic!("{name}: no ed25519 host key: {stdout}"));
            containers.push(Container {
                name: name.to_owned(),
                host_key: HostKey::parse(line).expect("an OpenSSH public key"),
            });
        }
        Self(Arc::new(Inner {
            containers,
            faults: Mutex::default(),
        }))
    }

    pub(crate) fn host_key(&self, name: &str) -> HostKey {
        self.0
            .containers
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.host_key.clone())
            .expect("a known container")
    }

    /// Has the logins come from `name`'s LAN address, as the gateway's `SSH_CONNECTION` tells:
    /// a `docker exec` has no SSH client, so nothing would renumber under the controller.
    pub(crate) fn controller_behind(&self, name: &str) {
        let mut faults = self.0.faults.lock().expect("faults");
        faults.controller = Some(name.to_owned());
    }

    /// Cuts `name` off once the tool applies a transaction on it.
    pub(crate) fn lose_after_apply(&self, name: &str) {
        let mut faults = self.0.faults.lock().expect("faults");
        faults.lose_after_apply.insert(name.to_owned());
    }
}

impl Inner {
    fn lost(&self, name: &str) -> bool {
        self.faults.lock().expect("faults").lost.contains(name)
    }

    /// The container that holds `address` on one of its interfaces now.
    async fn holding(&self, address: &str) -> Option<&Container> {
        let request = ExecRequest::new(["ip", "-4", "-o", "addr", "show"]);
        let needle = format!(" inet {address}/");
        for container in &self.containers {
            if self.lost(&container.name) {
                continue;
            }
            let Ok(output) = exec(&container.name, &request).await else {
                continue;
            };
            if String::from_utf8_lossy(&output.stdout).contains(&needle) {
                return Some(container);
            }
        }
        None
    }
}

fn disconnected(what: &str) -> Error {
    Error::Disconnected {
        what: what.to_owned(),
    }
}

impl Dial for DockerDial {
    fn banner<'a>(&'a self, _: &'a Target) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async { Ok(None) })
    }

    fn probe<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<HostKey>> {
        Box::pin(async move {
            let container = self
                .0
                .holding(&target.host)
                .await
                .ok_or_else(|| refused(target))?;
            Ok(container.host_key.clone())
        })
    }

    fn connect<'a>(
        &'a self,
        target: &'a Target,
        host_key: &'a HostKey,
        _credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>> {
        Box::pin(async move {
            let container = self
                .0
                .holding(&target.host)
                .await
                .ok_or_else(|| refused(target))?;
            if container.host_key != *host_key {
                return Err(Error::HostKeyMismatch {
                    target: target.to_string(),
                    presented: container.host_key.clone(),
                    pinned: host_key.clone(),
                });
            }
            Ok(Box::new(DockerRemote {
                dial: Arc::clone(&self.0),
                container: container.name.clone(),
            }) as Box<dyn Remote>)
        })
    }
}

fn refused(target: &Target) -> Error {
    Error::Connect {
        target: target.to_string(),
        source: std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "no container"),
    }
}

struct DockerRemote {
    dial: Arc<Inner>,
    container: String,
}

impl Remote for DockerRemote {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>> {
        Box::pin(async move {
            if self.dial.lost(&self.container) {
                return Err(disconnected("exec"));
            }
            let controller = self.dial.faults.lock().expect("faults").controller.clone();
            if let Some(host) = controller.filter(|_| request.argv == ["env"]) {
                return login_from(&host).await;
            }
            let output = exec(&self.container, &request).await?;
            let applied = request.argv.first().map(String::as_str) == Some(TOOL)
                && request.argv.get(1).map(String::as_str) == Some("apply")
                && output.status.success();
            if applied {
                let mut faults = self.dial.faults.lock().expect("faults");
                if faults.lose_after_apply.remove(&self.container) {
                    faults.lost.insert(self.container.clone());
                }
            }
            Ok(output)
        })
    }

    fn collect<'a>(&'a self, sets: &'a [Set]) -> BoxFuture<'a, Result<Collected>> {
        Box::pin(async move {
            if self.dial.lost(&self.container) {
                return Err(disconnected("collect"));
            }
            let call = serde_json::json!({"id": 1, "op": "collect", "args": {"sets": sets}});
            let request = ExecRequest::new(["ucode", "/tmp/uniwrt/agent/main.uc", "--sync"])
                .stdin(call.to_string().into_bytes())
                .timeout(COLLECT_TIMEOUT);
            let output = exec(&self.container, &request).await?.check("collect")?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let protocol = |why: String| Error::Protocol(format!("collect: {why}"));
            let line = stdout
                .lines()
                .last()
                .ok_or_else(|| protocol("no answer".into()))?;
            let answer: Value = serde_json::from_str(line).map_err(|e| protocol(e.to_string()))?;
            let ok = answer.get("ok").ok_or_else(|| protocol(line.to_owned()))?;
            serde_json::from_value(ok.clone()).map_err(|e| protocol(e.to_string()))
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// What `env` prints over a login from `container`'s LAN address, if it has one now.
async fn login_from(container: &str) -> Result<ExecOutput> {
    let request = ExecRequest::new(["ubus", "call", "network.interface.lan", "status"]);
    let output = exec(container, &request).await?;
    let status: Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let env = match status["ipv4-address"][0]["address"].as_str() {
        Some(address) => format!("USER=root\nSSH_CONNECTION={address} 50022 0.0.0.0 22\n"),
        None => "USER=root\n".to_owned(),
    };
    Ok(ExecOutput {
        status: ExitStatus::Code(0),
        stdout: env.into_bytes(),
        stderr: Vec::new(),
    })
}

/// Runs `request` in `container` through `docker exec`, bounded by its timeout and output limit.
pub(crate) async fn exec(container: &str, request: &ExecRequest) -> Result<ExecOutput> {
    let what = request.argv.join(" ");
    let mut child = Command::new("docker")
        .args(["exec", "-i", container])
        .args(&request.argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::Protocol(format!("docker exec: {e}")))?;
    let mut stdin = child.stdin.take().expect("a piped stdin");
    let input = request.stdin.clone().unwrap_or_default();
    let run = async move {
        stdin.write_all(&input).await?;
        drop(stdin);
        child.wait_with_output().await
    };
    let output = tokio::time::timeout(request.timeout, run)
        .await
        .map_err(|_| Error::Timeout {
            what: what.clone(),
            after: request.timeout,
        })?
        .map_err(|e| Error::Protocol(format!("docker exec: {e}")))?;
    if output.stdout.len() + output.stderr.len() > request.max_output {
        return Err(Error::Overflow {
            what,
            limit: request.max_output,
        });
    }
    let status = match output.status.code() {
        Some(code) => ExitStatus::Code(u32::try_from(code).unwrap_or(u32::MAX)),
        None => ExitStatus::Signal("killed".to_owned()),
    };
    Ok(ExecOutput {
        status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}
