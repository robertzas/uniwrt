//! [`SimLink`]: one simulated node behind [`NodeLink`]. It answers the agent's ops as the
//! event-loop agent does (`collect`, `subscribe`/`unsubscribe`, `set_interval`, `log_follow`,
//! `ping`, `scan`), keeps what they asked for in the node's [`Session`], and runs only the commands
//! adoption runs (`render::commands`).

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use serde::de::DeserializeOwned;
use serde_json::{Value, json, value::RawValue};
use tokio::sync::{broadcast, watch};
use uniwrt_node::{
    AgentEvent, AgentState, Error, ExecOutput, ExecRequest, ExecStream, LinkState, NodeLink,
    Result,
    proto::ops::{LogFollow, LogMode, Set, SetInterval, Topic},
};

use crate::{
    fleet::{Shared, lock},
    render,
};

/// A simulated node's link. Clones share it.
#[derive(Clone)]
pub struct SimLink {
    pub(crate) shared: Arc<Shared>,
    pub(crate) node: usize,
}

/// What the running agent was asked for; an agent that starts again starts empty.
#[derive(Debug, Default)]
pub(crate) struct Session {
    pub(crate) topics: BTreeSet<Topic>,
    /// By interval in ms: the sets it carries and when it is next due (world ms).
    pub(crate) ticks: BTreeMap<u64, Tick>,
    pub(crate) log: Option<LogFollowing>,
}

#[derive(Debug)]
pub(crate) struct Tick {
    pub(crate) sets: BTreeSet<Set>,
    pub(crate) due_ms: i64,
}

#[derive(Debug)]
pub(crate) struct LogFollowing {
    pub(crate) mode: LogMode,
    pub(crate) interval_ms: u64,
    pub(crate) due_ms: i64,
    /// The last logd id pushed.
    pub(crate) after_id: u64,
}

impl SimLink {
    /// Whether the node's `authorized_keys` holds `line`, as adoption installs it.
    pub fn trusts(&self, line: &str) -> bool {
        lock(&self.shared.nodes[self.node].disk)
            .authorized_keys
            .iter()
            .any(|key| key == line)
    }

    /// Adds `line` to the node's `authorized_keys`, as adoption does: demo mode's nodes
    /// adopted at start trust the controller's key.
    pub fn authorize(&self, line: &str) {
        let keys = &mut lock(&self.shared.nodes[self.node].disk).authorized_keys;
        if !keys.iter().any(|key| key == line) {
            keys.push(line.to_owned());
        }
    }

    /// Whether `password` is root's: the empty one on a factory-default node, which dropbear
    /// lets in without one.
    pub fn root_password_is(&self, password: &str) -> bool {
        lock(&self.shared.nodes[self.node].disk).root_password == password
    }

    /// The addresses the node answers at: its own and, on the gateway, every address of its
    /// `lan` (two while a renumbering bridges the old subnet and the new, D-081).
    pub fn hosts(&self) -> Vec<String> {
        let world = self.shared.world();
        let node = &world.nodes[self.node];
        let mut hosts = vec![node.ip];
        if node.gateway {
            let disk = lock(&self.shared.nodes[self.node].disk);
            for address in render::config::lan_addresses(&disk.tool.files) {
                if !hosts.contains(&address) {
                    hosts.push(address);
                }
            }
        }
        hosts.iter().map(ToString::to_string).collect()
    }

    fn answer(&self, op: &str, args: &RawValue) -> Result<Value> {
        if !self.running() {
            return Err(Error::AgentUnavailable {
                reason: "the simulated node is down".to_owned(),
            });
        }
        let mut world = self.shared.world();
        let mut session = self.shared.session(self.node);
        let now_ms = world.now_ms();
        Ok(match op {
            "ping" => json!({"ts": now_ms}),
            "collect" => {
                let sets: Vec<Set> = field(op, args, "sets")?;
                let disk = lock(&self.shared.nodes[self.node].disk);
                render::collect(&world, self.node, &sets, &disk.tool.files, &disk.bss_off())
            }
            "read_file" => self.read_file(op, args)?,
            "subscribe" | "unsubscribe" => {
                let topics: Vec<Topic> = field(op, args, "topics")?;
                if op == "subscribe" {
                    session.topics.extend(topics);
                } else {
                    session.topics.retain(|topic| !topics.contains(topic));
                }
                json!({"topics": session.topics})
            }
            "set_interval" => {
                let interval_ms: u64 = field(op, args, "interval_ms")?;
                let sets: Vec<Set> = field(op, args, "sets")?;
                if interval_ms != 0
                    && !(SetInterval::MIN_MS..=SetInterval::MAX_MS).contains(&interval_ms)
                {
                    return Err(bad_request(op, "interval_ms is out of range"));
                }
                for tick in session.ticks.values_mut() {
                    tick.sets.retain(|set| !sets.contains(set));
                }
                session.ticks.retain(|_, tick| !tick.sets.is_empty());
                if interval_ms > 0 && !sets.is_empty() {
                    session
                        .ticks
                        .entry(interval_ms)
                        .or_insert_with(|| Tick {
                            sets: BTreeSet::new(),
                            due_ms: now_ms + interval_ms as i64,
                        })
                        .sets
                        .extend(sets);
                }
                let intervals: BTreeMap<_, _> = session
                    .ticks
                    .iter()
                    .flat_map(|(&ms, tick)| tick.sets.iter().map(move |set| (set.as_str(), ms)))
                    .collect();
                json!({"intervals": intervals})
            }
            "log_follow" => {
                let mode: LogMode = field(op, args, "mode")?;
                let backfill: u32 = field(op, args, "backfill")?;
                let interval_ms: u64 = field(op, args, "interval_ms")?;
                session.log = (mode != LogMode::Off).then(|| {
                    let records = &world.nodes[self.node].logd.records;
                    let backfill = backfill.min(LogFollow::MAX_BACKFILL) as usize;
                    let after_id = records
                        .len()
                        .checked_sub(backfill + 1)
                        .and_then(|first| records.get(first))
                        .map_or(0, |record| record.id);
                    LogFollowing {
                        mode,
                        interval_ms: interval_ms.max(100),
                        // The backfill goes out with the next second.
                        due_ms: now_ms,
                        after_id,
                    }
                });
                json!({"mode": mode})
            }
            "bss" => self.bss(&world, op, args)?,
            "ban" => self.ban(&mut world, op, args)?,
            "scan" => {
                let ifname: String = field(op, args, "ifname")?;
                render::scan::scan(&world, self.node, &ifname).ok_or_else(|| Error::Remote {
                    op: op.to_owned(),
                    code: "not_found".to_owned(),
                    msg: format!("the radio cannot scan: no such device {ifname}"),
                })?
            }
            _ => return Err(bad_request(op, "unknown op")),
        })
    }

    /// `bss`: the BSSes of a phy to keep off, until `wireless` changes (a reload brings them
    /// back), as `node-agent/bss.uc` has hostapd do.
    fn bss(&self, world: &crate::world::World, op: &str, args: &RawValue) -> Result<Value> {
        let phy: String = field(op, args, "phy")?;
        let off: Vec<String> = field(op, args, "off")?;
        let node = &world.nodes[self.node];
        let Some((index, band)) = node
            .radios
            .iter()
            .map(|radio| (radio.band.index(), radio.band))
            .find(|(index, _)| format!("phy{index}") == phy)
        else {
            return Err(Error::Remote {
                op: op.to_owned(),
                code: "not_found".to_owned(),
                msg: format!("netifd has no hostapd config of {phy}"),
            });
        };
        let mut disk = lock(&self.shared.nodes[self.node].disk);
        let aps = render::config::aps(&disk.tool.files);
        let mine = render::wifi::ap_ifaces(&aps, index, band);
        let mut now_off = disk.bss_off();
        now_off.retain(|ifname| !mine.iter().any(|(_, i, _)| i == ifname));
        let (dropped, kept): (Vec<_>, Vec<_>) = mine
            .into_iter()
            .map(|(_, ifname, _)| ifname)
            .partition(|ifname| off.contains(ifname));
        now_off.extend(dropped.iter().cloned());
        let wireless = disk.tool.files.get("wireless").cloned().unwrap_or_default();
        disk.bss_off = Some((wireless, now_off));
        Ok(json!({"radios": [{"radio": null, "on": kept, "off": dropped}]}))
    }

    /// `ban`: hostapd's bans on the node's access points (`node-agent/ban.uc`), which the world
    /// keeps its clients to.
    fn ban(&self, world: &mut crate::world::World, op: &str, args: &RawValue) -> Result<Value> {
        #[derive(serde::Deserialize)]
        struct Entry {
            addr: uniwrt_core::MacAddr,
            ms: u32,
            #[serde(default)]
            kick: bool,
        }
        let bans: Vec<Entry> = field(op, args, "bans")?;
        let off = lock(&self.shared.nodes[self.node].disk).bss_off();
        let bsses: Vec<&str> = ["phy0-ap0", "phy1-ap0"]
            .into_iter()
            .filter(|ifname| !off.contains(*ifname))
            .collect();
        let kicked: Vec<Value> = bans
            .iter()
            .filter_map(|b| {
                let kicked_from = world.ban(self.node, b.addr, b.ms, b.kick)?;
                Some(json!({"ifname": kicked_from.ap_iface(), "addr": b.addr.to_string()}))
            })
            .collect();
        Ok(json!({"bsses": bsses, "kicked": kicked, "failed": []}))
    }

    /// `read_file` of a package in `/etc/config`, which is all the controller reads so.
    fn read_file(&self, op: &str, args: &RawValue) -> Result<Value> {
        let path: String = field(op, args, "path")?;
        let disk = lock(&self.shared.nodes[self.node].disk);
        let file = path
            .strip_prefix("/etc/config/")
            .and_then(|name| disk.tool.files.get(name));
        match file {
            Some(data) => Ok(json!({"data": data, "truncated": false})),
            None => Err(Error::Remote {
                op: op.to_owned(),
                code: "not_found".to_owned(),
                msg: format!("{path}: No such file or directory"),
            }),
        }
    }

    fn run(&self, request: &ExecRequest) -> Result<ExecOutput> {
        if !self.running() {
            return Err(Error::Disconnected {
                what: "exec".to_owned(),
            });
        }
        let mut world = self.shared.world();
        let mut disk = lock(&self.shared.nodes[self.node].disk);
        let output =
            render::commands::exec(&world.nodes[self.node], request, &mut disk, world.now_ms());
        crate::fleet::follow_bounces(&mut world, self.node, &mut disk);
        Ok(output)
    }

    fn running(&self) -> bool {
        matches!(
            &*self.shared.nodes[self.node].state.borrow(),
            LinkState::Up {
                agent: AgentState::Running(_),
                ..
            }
        )
    }
}

fn field<T: DeserializeOwned>(op: &str, args: &RawValue, name: &str) -> Result<T> {
    let args: Value = serde_json::from_str(args.get())
        .map_err(|err| bad_request(op, &format!("arguments: {err}")))?;
    serde_json::from_value(args[name].clone())
        .map_err(|err| bad_request(op, &format!("{name}: {err}")))
}

fn bad_request(op: &str, msg: &str) -> Error {
    Error::Remote {
        op: op.to_owned(),
        code: "bad_request".to_owned(),
        msg: msg.to_owned(),
    }
}

fn unsupported(op: &str) -> Error {
    Error::Remote {
        op: op.to_owned(),
        code: "unsupported".to_owned(),
        msg: "a simulated node runs no commands".to_owned(),
    }
}

impl NodeLink for SimLink {
    fn call(
        &self,
        op: &str,
        args: &RawValue,
        _timeout: Duration,
    ) -> impl Future<Output = Result<Box<RawValue>>> + Send {
        let answer = self.answer(op, args).and_then(|answer| {
            serde_json::value::to_raw_value(&answer)
                .map_err(|err| Error::Protocol(format!("cannot encode the answer: {err}")))
        });
        std::future::ready(answer)
    }

    fn exec(&self, request: ExecRequest) -> impl Future<Output = Result<ExecOutput>> + Send {
        std::future::ready(self.run(&request))
    }

    fn stream(&self, _argv: Vec<String>) -> impl Future<Output = Result<ExecStream>> + Send {
        std::future::ready(Err(unsupported("stream")))
    }

    fn upload(
        &self,
        _path: &str,
        _data: Bytes,
        _mode: u32,
    ) -> impl Future<Output = Result<()>> + Send {
        std::future::ready(Err(unsupported("upload")))
    }

    fn events(&self) -> broadcast::Receiver<AgentEvent> {
        self.shared.nodes[self.node].pushes.subscribe()
    }

    fn state(&self) -> watch::Receiver<LinkState> {
        self.shared.nodes[self.node].state.subscribe()
    }
}
