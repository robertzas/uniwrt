//! A node of the harness: an OpenWrt container that the tests reach through `docker exec`.
//! Values reach the container's shell as arguments (`sh -c '…' sh <value>`), never inside the
//! script's text.

use std::{
    collections::BTreeMap,
    io::Write as _,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use uniwrt_model::facts::NodeFacts;
use uniwrt_node::{facts, proto::ops::Collected};
use uniwrt_render::Rendered;
use uniwrt_uci::Package;

/// What a command printed, and whether it succeeded.
#[derive(Debug)]
pub struct Run {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub container: String,
}

impl Node {
    pub fn new(container: impl Into<String>) -> Self {
        Self {
            container: container.into(),
        }
    }

    /// Runs `script` in the container's shell, with `args` as `$1`…, and `input` on stdin.
    pub fn run(&self, script: &str, args: &[&str], input: &[u8]) -> Run {
        let mut child = Command::new("docker")
            .args(["exec", "-i", &self.container, "sh", "-c", script, "sh"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("docker runs");
        let mut stdin = child.stdin.take().expect("a piped stdin");
        stdin
            .write_all(input)
            .expect("the container reads its input");
        drop(stdin);
        let output = child.wait_with_output().expect("docker exec finishes");
        Run {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Runs `script` and returns its stdout; panics with its output when it fails.
    pub fn sh(&self, script: &str, args: &[&str]) -> String {
        let run = self.run(script, args, &[]);
        assert!(
            run.ok,
            "{}: `{script}` {args:?} failed:\n{}{}",
            self.container, run.stdout, run.stderr
        );
        run.stdout
    }

    /// The node's facts, from the working tree's agent (installed by scripts/test-system.sh).
    pub fn facts(&self) -> NodeFacts {
        let request = br#"{"id":1,"op":"collect","args":{}}"#;
        let run = self.run("ucode /tmp/uniwrt/agent/main.uc --sync", &[], request);
        assert!(run.ok, "{}: collect failed: {}", self.container, run.stderr);
        let line = run.stdout.lines().last().expect("the agent answers");
        let answer: Value = serde_json::from_str(line).expect("a JSON answer");
        let collected: Collected =
            serde_json::from_value(answer["ok"].clone()).expect("a collect answer");
        assert_eq!(collected.errors, BTreeMap::new(), "{}", self.container);
        let mut node = NodeFacts::default();
        let errors = facts::apply(&collected, &mut node);
        assert!(errors.is_empty(), "{}: {errors:?}", self.container);
        node
    }

    /// The managed packages the node holds now, by name.
    pub fn packages(&self) -> BTreeMap<String, Package> {
        uniwrt_render::RENDERABLE
            .iter()
            .map(|package| package.name())
            .filter_map(|name| {
                let run = self.run(r#"cat "/etc/config/$1""#, &[name], &[]);
                run.ok.then(|| {
                    let package = uniwrt_uci::parse(name, &run.stdout)
                        .unwrap_or_else(|e| panic!("{}: /etc/config/{name}: {e}", self.container));
                    (name.to_owned(), package)
                })
            })
            .collect()
    }

    /// Applies `rendered` as a node would after a commit: stops and disables the services
    /// that go off, writes each package and checks that `uci` reads back exactly what was
    /// written, enables the services that stay on, runs `reload_config` and restarts them.
    ///
    /// The services go off first: procd fires a service's reload triggers a moment after
    /// `reload_config`, and dnsmasq's reload starts it again, enabled or not; a stopped service
    /// has no triggers left.
    pub fn apply(&self, rendered: &Rendered) {
        for (&service, _) in rendered.services.iter().filter(|(_, on)| !**on) {
            self.sh(
                r#"/etc/init.d/"$1" disable; /etc/init.d/"$1" stop"#,
                &[service],
            );
        }
        for package in rendered.packages.values() {
            let name = package.name.as_str();
            let write = self.run(
                r#"cat >"/etc/config/$1""#,
                &[name],
                package.to_file().as_bytes(),
            );
            assert!(
                write.ok,
                "{}: writing {name}: {}",
                self.container, write.stderr
            );
            let export = self.sh(r#"uci export "$1""#, &[name]);
            let read = uniwrt_uci::parse_export(&export)
                .unwrap_or_else(|e| panic!("{}: uci export {name}: {e}", self.container));
            assert_eq!(read.len(), 1, "{}: uci export {name}", self.container);
            let diff = uniwrt_uci::diff(package, &read[0]);
            assert!(
                diff.is_empty(),
                "{}: uci reads {name} otherwise than it was written: {diff:?}",
                self.container
            );
        }
        let on: Vec<&str> = rendered
            .services
            .iter()
            .filter(|(_, on)| **on)
            .map(|(&service, _)| service)
            .collect();
        for &service in &on {
            self.sh(r#"/etc/init.d/"$1" enable"#, &[service]);
        }
        self.sh("reload_config", &[]);
        // procd runs the reload triggers PROCD_RELOAD_DELAY (1 s) later: let them run first,
        // so the restarts below come last and the checks never see the previous config.
        thread::sleep(Duration::from_secs(2));
        for &service in &on {
            // fw4 refuses to load rules the kernel can't take.
            if service == "firewall" && !crate::needs("nftables") {
                continue;
            }
            self.sh(r#"/etc/init.d/"$1" restart"#, &[service]);
        }
    }

    /// `network.interface.<name> status`, or `None` when netifd has no such interface.
    pub fn interface(&self, name: &str) -> Option<Value> {
        let run = self.run(r#"ubus call "network.interface.$1" status"#, &[name], &[]);
        run.ok
            .then(|| serde_json::from_str(&run.stdout).expect("ubus prints JSON"))
    }

    /// Waits until `ready` holds for the interface's status, and returns that status.
    pub fn wait_interface(
        &self,
        name: &str,
        timeout: Duration,
        ready: impl Fn(&Value) -> bool,
    ) -> Value {
        let start = Instant::now();
        loop {
            let status = self.interface(name);
            if let Some(status) = status.as_ref().filter(|s| ready(s)) {
                return status.clone();
            }
            if start.elapsed() > timeout {
                let log = self.sh("logread | tail -n 60", &[]);
                panic!(
                    "{}: interface {name} did not come up in {timeout:?}: {status:#?}\n{log}",
                    self.container
                );
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// Each bridge port's VLANs (`bridge vlan show`): the VLAN ids, with `P` for the PVID and
    /// `U` for untagged egress, as `"1PU"`, `"30"`.
    pub fn bridge_vlans(&self) -> BTreeMap<String, Vec<String>> {
        let json: Value =
            serde_json::from_str(&self.sh("bridge -j vlan show", &[])).expect("bridge prints JSON");
        let mut ports = BTreeMap::new();
        for port in json.as_array().into_iter().flatten() {
            let name = port["ifname"].as_str().unwrap_or_default().to_owned();
            let vlans = port["vlans"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|vlan| {
                    let flags: Vec<&str> = vlan["flags"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect();
                    let mut text = vlan["vlan"].to_string();
                    if flags.contains(&"PVID") {
                        text.push('P');
                    }
                    if flags.contains(&"Egress Untagged") {
                        text.push('U');
                    }
                    text
                })
                .collect();
            ports.insert(name, vlans);
        }
        ports
    }
}

/// The interface is up with an IPv4 address.
pub fn has_ipv4(status: &Value) -> bool {
    status["up"] == true && !ipv4(status).is_empty()
}

/// The interface's IPv4 addresses, as `address/mask`.
pub fn ipv4(status: &Value) -> Vec<String> {
    status["ipv4-address"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            format!(
                "{}/{}",
                a["address"].as_str().unwrap_or_default(),
                a["mask"]
            )
        })
        .collect()
}
