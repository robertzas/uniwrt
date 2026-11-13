//! The transaction tool as a node runs it (`node-agent/txn/txn`; features/lifecycle.md, Render
//! → diff → apply, D-069), with the node's `/etc/config` files and init scripts: the simulated
//! fleet runs it, and the jobs' tests drive it through their scripted nodes.
//!
//! [`Tool::exec`] answers the tool's commands with its replies and error codes, the
//! footprint's status, install and removal, and the reads of the current packages and services
//! ([`uniwrt_node::txn::current`]). Time is the caller's ([`Now`]): the dead-man timer expires
//! when a command runs after its deadline, and a node that booted since its apply rolls back
//! (reason `boot`). The reload is instant, so an apply is `applied` at once. `check` takes any
//! staged file: the sim has no `uci` to parse them with.

use std::collections::BTreeMap;

use serde_json::json;
use uniwrt_node::{
    ExecOutput, ExitStatus,
    txn::{self, Footprint, MAX_TIMEOUT, MIN_TIMEOUT, TOOL, code, current},
};

/// The node's clocks when a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    /// Seconds since the node booted.
    pub uptime_s: u64,
    pub unix_s: i64,
    /// When the node booted (Unix ms): a change means it rebooted.
    pub booted_ms: i64,
}

/// The transaction tool and the files it manages, as they survive reboots.
#[derive(Debug, Clone, Default)]
pub struct Tool {
    /// The footprint's version, once management is on.
    pub footprint: Option<String>,
    /// `/etc/config`, by package.
    pub files: BTreeMap<String, String>,
    /// The installed init scripts, and whether each starts at boot.
    pub services: BTreeMap<String, bool>,
    /// The ports each `bounce` named, in order.
    pub bounced: Vec<Vec<String>>,
    /// The radios each live `apply` named (their channels move by a switch, D-113), in order.
    pub live: Vec<Vec<String>>,
    /// The radios each rollback of a live apply restarted, in order.
    pub restarted: Vec<Vec<String>>,
    stage: Option<Stage>,
    open: Option<Open>,
    last: Option<serde_json::Value>,
    confirmed: Option<Confirmed>,
}

/// The last transaction confirmed: when, and the packages it wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmed {
    pub unix_s: i64,
    pub packages: Vec<String>,
}

#[derive(Debug, Clone)]
struct Stage {
    id: String,
    services: Vec<(String, bool)>,
    packages: BTreeMap<String, String>,
    checked: bool,
}

#[derive(Debug, Clone)]
struct Open {
    id: String,
    deadline_s: u64,
    booted_ms: i64,
    /// The packages it replaced, `None` for those the node lacked.
    files: BTreeMap<String, Option<String>>,
    services: BTreeMap<String, bool>,
    /// The ports it bounced, which a rollback bounces again.
    bounced: Option<Vec<String>>,
    /// The radios a live apply named, which a rollback restarts.
    radios: Vec<String>,
}

type Reply = Result<serde_json::Value, (&'static str, String)>;

impl Tool {
    /// A node with OpenWrt's stock services, all enabled.
    pub fn stock() -> Self {
        Self {
            services: [
                "dnsmasq", "dropbear", "firewall", "network", "odhcpd", "system",
            ]
            .into_iter()
            .map(|name| (name.to_owned(), true))
            .collect(),
            ..Self::default()
        }
    }

    /// Answers `argv` (with `stdin`) if it is one of the tool's commands or reads.
    pub fn exec(&mut self, argv: &[String], stdin: Option<&[u8]>, now: Now) -> Option<ExecOutput> {
        self.expire(now);
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        let reply = match words.as_slice() {
            ["sh", "-c", script] if *script == script_of(&txn::status().request) => {
                Ok(self.status(now))
            }
            ["sh", "-c", script] if *script == script_of(&Footprint::get().install().request) => {
                self.install()
            }
            ["sh", "-c", script] if *script == script_of(&txn::uninstall().request) => {
                self.uninstall()
            }
            ["sh", "-c", script, "sh", names @ ..] if *script == current::PACKAGES_SCRIPT => {
                return Some(ok_raw(self.read_packages(names)));
            }
            ["sh", "-c", script, "sh", names @ ..] if *script == current::SERVICES_SCRIPT => {
                return Some(ok_raw(self.read_services(names).into_bytes()));
            }
            [tool, command, rest @ ..] if *tool == TOOL && self.footprint.is_some() => {
                self.command(command, rest, stdin, now)
            }
            _ => return None,
        };
        Some(match reply {
            Ok(value) => ok_raw(format!("{}\n", json!({ "ok": value })).into_bytes()),
            Err((code, msg)) => ExecOutput {
                status: ExitStatus::Code(1),
                stdout: format!("{}\n", json!({"err": {"code": code, "msg": msg}})).into_bytes(),
                stderr: Vec::new(),
            },
        })
    }

    /// The node's own timer: rolls back an open transaction nobody confirmed in time, whether
    /// or not anyone asks.
    pub fn tick(&mut self, now: Now) {
        self.expire(now);
    }

    /// The open transaction's id, if any.
    pub fn open(&self) -> Option<&str> {
        self.open.as_ref().map(|open| open.id.as_str())
    }

    /// The last transaction confirmed, if any.
    pub fn confirmed(&self) -> Option<&Confirmed> {
        self.confirmed.as_ref()
    }

    fn command(&mut self, command: &str, args: &[&str], stdin: Option<&[u8]>, now: Now) -> Reply {
        match (command, args) {
            ("stage", [id, services @ ..]) => self.stage(id, services),
            ("put", [id, package]) => self.put(id, package, stdin.unwrap_or_default()),
            ("check", [id]) => self.check(id),
            ("apply", [id, seconds, radios @ ..]) => self.apply(id, seconds, radios, now),
            ("extend", [id, seconds]) => {
                let seconds = timeout(seconds)?;
                self.require_open(id)?.deadline_s = now.uptime_s + seconds;
                Ok(json!({ "timeout_s": seconds }))
            }
            ("bounce", [seconds, ports @ ..]) if !ports.is_empty() => {
                if !(1..=30).contains(&seconds.parse::<u64>().unwrap_or(0)) {
                    return Err((code::BAD_REQUEST, "the seconds are 1 to 30".to_owned()));
                }
                // The sim's open transaction is applied as soon as it is open.
                let Some(open) = &mut self.open else {
                    return Err((code::BAD_REQUEST, "no transaction is open".to_owned()));
                };
                let ports: Vec<String> = ports.iter().map(|p| (*p).to_owned()).collect();
                let count = ports.len();
                open.bounced = Some(ports.clone());
                self.bounced.push(ports);
                Ok(json!({ "ports": count }))
            }
            ("confirm", [id]) => {
                self.require_open(id)?;
                self.finish("confirmed", None, now);
                Ok(json!({}))
            }
            ("rollback", [id]) => {
                self.require_open(id)?;
                self.roll_back("requested", now);
                Ok(json!({}))
            }
            _ => Err((code::BAD_REQUEST, format!("unknown command: {command}"))),
        }
    }

    fn stage(&mut self, id: &str, services: &[&str]) -> Reply {
        if let Some(open) = &self.open {
            return Err((code::BUSY, format!("transaction {} is open", open.id)));
        }
        let services = services
            .iter()
            .map(|arg| match arg.split_once('=') {
                Some((name, "on")) => Ok((name.to_owned(), true)),
                Some((name, "off")) => Ok((name.to_owned(), false)),
                _ => Err((code::BAD_REQUEST, format!("a service is on or off: {arg}"))),
            })
            .collect::<Result<_, _>>()?;
        self.stage = Some(Stage {
            id: (*id).to_owned(),
            services,
            packages: BTreeMap::new(),
            checked: false,
        });
        Ok(json!({}))
    }

    fn staged(&mut self, id: &str) -> Result<&mut Stage, (&'static str, String)> {
        self.stage
            .as_mut()
            .filter(|stage| stage.id == id)
            .ok_or_else(|| (code::NOT_FOUND, format!("transaction {id} is not staged")))
    }

    fn put(&mut self, id: &str, package: &str, content: &[u8]) -> Reply {
        let stage = self.staged(id)?;
        let text = String::from_utf8_lossy(content).into_owned();
        stage.packages.insert(package.to_owned(), text);
        Ok(json!({ "bytes": content.len() }))
    }

    fn check(&mut self, id: &str) -> Reply {
        let installed = self.services.clone();
        let stage = self.staged(id)?;
        if let Some((name, _)) = stage
            .services
            .iter()
            .find(|(name, on)| *on && !installed.contains_key(name))
        {
            return Err((code::INVALID, format!("service {name} is not installed")));
        }
        if stage.packages.is_empty() && stage.services.is_empty() {
            return Err((code::BAD_REQUEST, "nothing is staged".to_owned()));
        }
        stage.checked = true;
        Ok(json!({ "packages": stage.packages.keys().collect::<Vec<_>>() }))
    }

    fn apply(&mut self, id: &str, seconds: &str, radios: &[&str], now: Now) -> Reply {
        let seconds = timeout(seconds)?;
        if let Some(open) = &self.open {
            return Err((code::BUSY, format!("transaction {} is open", open.id)));
        }
        let stage = match self.stage.take() {
            Some(stage) if stage.id == id && stage.checked => stage,
            other => {
                self.stage = other;
                return Err((
                    code::BAD_REQUEST,
                    format!("transaction {id} is not staged and checked"),
                ));
            }
        };
        let files = stage
            .packages
            .keys()
            .map(|name| (name.clone(), self.files.get(name).cloned()))
            .collect();
        let services = stage
            .services
            .iter()
            .filter_map(|(name, _)| Some((name.clone(), *self.services.get(name)?)))
            .collect();
        self.files.extend(stage.packages);
        for (name, on) in stage.services {
            if let Some(enabled) = self.services.get_mut(&name) {
                *enabled = on;
            }
        }
        self.open = Some(Open {
            id: id.to_owned(),
            deadline_s: now.uptime_s + seconds,
            booted_ms: now.booted_ms,
            files,
            services,
            bounced: None,
            radios: radios.iter().map(|r| (*r).to_owned()).collect(),
        });
        if !radios.is_empty() {
            self.live
                .push(radios.iter().map(|r| (*r).to_owned()).collect());
        }
        Ok(json!({ "timeout_s": seconds }))
    }

    fn require_open(&mut self, id: &str) -> Result<&mut Open, (&'static str, String)> {
        let ended = self.last.as_ref().filter(|last| last["id"] == id);
        match (&mut self.open, ended) {
            (Some(open), _) if open.id == id => Ok(open),
            (_, Some(last)) => Err((
                code::EXPIRED,
                format!("transaction {id} is no longer open: {last}"),
            )),
            _ => Err((code::NOT_FOUND, format!("transaction {id} is not open"))),
        }
    }

    /// Rolls back an open transaction whose node rebooted or whose deadline passed.
    fn expire(&mut self, now: Now) {
        match &self.open {
            Some(open) if open.booted_ms != now.booted_ms => self.roll_back("boot", now),
            Some(open) if now.uptime_s >= open.deadline_s => self.roll_back("timeout", now),
            _ => {}
        }
    }

    fn roll_back(&mut self, reason: &str, now: Now) {
        let Some(open) = self.open.take() else { return };
        for (name, file) in &open.files {
            match file {
                Some(content) => self.files.insert(name.clone(), content.clone()),
                None => self.files.remove(name),
            };
        }
        self.services.extend(open.services.clone());
        if reason != "boot"
            && let Some(ports) = &open.bounced
        {
            self.bounced.push(ports.clone());
        }
        if reason != "boot" && !open.radios.is_empty() {
            self.restarted.push(open.radios.clone());
        }
        self.open = Some(open);
        self.finish("rolled_back", Some(reason), now);
    }

    fn finish(&mut self, outcome: &str, reason: Option<&str>, now: Now) {
        if let Some(open) = self.open.take() {
            if outcome == "confirmed" {
                self.confirmed = Some(Confirmed {
                    unix_s: now.unix_s,
                    packages: open.files.keys().cloned().collect(),
                });
            }
            self.last = Some(json!({
                "id": open.id, "outcome": outcome, "reason": reason, "at": now.unix_s,
            }));
        }
    }

    fn status(&self, now: Now) -> serde_json::Value {
        let Some(version) = &self.footprint else {
            return serde_json::Value::Null;
        };
        let open = self.open.as_ref().map(|open| {
            json!({
                "id": open.id,
                "state": "applied",
                "remaining_s": open.deadline_s.saturating_sub(now.uptime_s),
            })
        });
        json!({ "version": version, "open": open, "last": self.last })
    }

    fn install(&mut self) -> Reply {
        if let Some(open) = &self.open {
            return Err((code::BUSY, format!("transaction {} is open", open.id)));
        }
        self.footprint = Some(Footprint::get().version().to_owned());
        Ok(json!({}))
    }

    fn uninstall(&mut self) -> Reply {
        if let Some(open) = &self.open {
            return Err((code::BUSY, format!("transaction {} is open", open.id)));
        }
        self.footprint = None;
        self.stage = None;
        self.last = None;
        Ok(json!({}))
    }

    fn read_packages(&self, names: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for name in names {
            match self.files.get(*name) {
                Some(file) => {
                    out.extend(format!("{name} {}\n", file.len()).into_bytes());
                    out.extend(file.as_bytes());
                }
                None => out.extend(format!("{name} -\n").into_bytes()),
            }
        }
        out
    }

    fn read_services(&self, names: &[&str]) -> String {
        let mut out = String::new();
        for name in names {
            let state = match self.services.get(*name) {
                Some(true) => "on",
                Some(false) => "off",
                None => "absent",
            };
            out.push_str(name);
            out.push(' ');
            out.push_str(state);
            out.push('\n');
        }
        out
    }
}

fn timeout(seconds: &str) -> Result<u64, (&'static str, String)> {
    seconds
        .parse::<u64>()
        .ok()
        .filter(|s| (MIN_TIMEOUT.as_secs()..=MAX_TIMEOUT.as_secs()).contains(s))
        .ok_or_else(|| {
            (
                code::BAD_REQUEST,
                format!(
                    "the timeout is {} to {} seconds",
                    MIN_TIMEOUT.as_secs(),
                    MAX_TIMEOUT.as_secs()
                ),
            )
        })
}

/// The script of an `sh -c` command.
fn script_of(request: &uniwrt_node::ExecRequest) -> &str {
    request.argv.get(2).map_or("", String::as_str)
}

fn ok_raw(stdout: Vec<u8>) -> ExecOutput {
    ExecOutput {
        status: ExitStatus::Code(0),
        stdout,
        stderr: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
