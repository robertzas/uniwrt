//! [`SimLink`]: one simulated node behind [`NodeLink`]. It answers the agent's ops as the
//! event-loop agent does (`collect`, `subscribe`/`unsubscribe`, `set_interval`, `log_follow`,
//! `ping`), keeps what they asked for in the node's [`Session`], and runs no commands.

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

use crate::{fleet::Shared, render};

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
    fn answer(&self, op: &str, args: &RawValue) -> Result<Value> {
        let channel = &self.shared.nodes[self.node];
        if !matches!(
            &*channel.state.borrow(),
            LinkState::Up {
                agent: AgentState::Running(_),
                ..
            }
        ) {
            return Err(Error::AgentUnavailable {
                reason: "the simulated node is down".to_owned(),
            });
        }
        let world = self.shared.world();
        let mut session = self.shared.session(self.node);
        let now_ms = world.now_ms();
        Ok(match op {
            "ping" => json!({"ts": now_ms}),
            "collect" => {
                let sets: Vec<Set> = field(op, args, "sets")?;
                render::collect(&world, self.node, &sets)
            }
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
            _ => return Err(bad_request(op, "unknown op")),
        })
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

    fn exec(&self, _request: ExecRequest) -> impl Future<Output = Result<ExecOutput>> + Send {
        std::future::ready(Err(unsupported("exec")))
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
