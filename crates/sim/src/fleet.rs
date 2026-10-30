//! [`SimFleet`]: the world, a link per node, and the driver task that advances the world one
//! second per second of tokio's clock. Each second it pushes what the agents would: the
//! notifications of the topics they subscribed to, the ticks that are due, and the log
//! batches of a follow. A node that reboots drops its link, is retried with backoff while it
//! is down, and comes back with a new agent that remembers nothing.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use serde_json::value::to_raw_value;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};
use uniwrt_core::{Clock, MacAddr};
use uniwrt_model::node::NodeRole;
use uniwrt_node::{
    AgentEvent, AgentState, LinkState,
    proto::ops::{LogMode, Set},
};

use crate::{
    SimConfig,
    link::{Session, SimLink},
    render,
    world::{Happening, What, World},
};

/// Pushes a receiver may fall behind by before it skips ahead.
const PUSH_CAPACITY: usize = 256;
/// Seconds between attempts to reach a node that is down, at first and at most.
const RETRY_MIN_S: u64 = 5;
const RETRY_MAX_S: u64 = 30;

/// A node of the fleet, as it would be adopted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimNode {
    pub name: String,
    pub mac: MacAddr,
    pub role: NodeRole,
    pub model: String,
    pub board_name: String,
    /// Its LAN address.
    pub host: String,
}

/// The simulated fleet. Dropping it stops the world; its links then stay where they were.
pub struct SimFleet {
    shared: Arc<Shared>,
    driver: JoinHandle<()>,
}

pub(crate) struct Shared {
    world: Mutex<World>,
    pub(crate) nodes: Vec<Channel>,
}

pub(crate) struct Channel {
    pub(crate) state: watch::Sender<LinkState>,
    pub(crate) pushes: broadcast::Sender<AgentEvent>,
    session: Mutex<Session>,
    connection: Mutex<Connection>,
}

/// Where the controller's connection to a node stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Connection {
    Up,
    /// Down; the next attempt is at second `at`.
    Retrying {
        at: u64,
        attempt: u32,
    },
    Connecting {
        attempt: u32,
    },
    /// Logged in; the agent is starting.
    Starting,
}

impl SimFleet {
    /// Builds the world `config` describes, starting at `clock`'s now, and runs it on tokio's
    /// clock. Must be called within a tokio runtime.
    pub fn spawn(config: &SimConfig, clock: &dyn Clock) -> Self {
        let world = World::new(config, clock.now());
        let nodes = (0..world.nodes.len())
            .map(|node| Channel {
                state: watch::channel(running(&world, node)).0,
                pushes: broadcast::channel(PUSH_CAPACITY).0,
                session: Mutex::new(Session::default()),
                connection: Mutex::new(Connection::Up),
            })
            .collect();
        let shared = Arc::new(Shared {
            world: Mutex::new(world),
            nodes,
        });
        let driver = tokio::spawn({
            let shared = Arc::clone(&shared);
            async move {
                let mut second = tokio::time::interval(Duration::from_secs(1));
                second.tick().await;
                loop {
                    second.tick().await;
                    shared.step();
                }
            }
        });
        Self { shared, driver }
    }

    pub fn nodes(&self) -> Vec<SimNode> {
        let world = self.shared.world();
        world
            .nodes
            .iter()
            .map(|node| SimNode {
                name: node.name.clone(),
                mac: node.mac,
                role: if node.gateway {
                    NodeRole::Gateway
                } else {
                    NodeRole::Ap
                },
                model: node.model.name.to_owned(),
                board_name: node.model.board_name.to_owned(),
                host: node.ip.to_string(),
            })
            .collect()
    }

    /// The link of the `index`-th node of [`nodes`](Self::nodes).
    pub fn link(&self, index: usize) -> Option<SimLink> {
        (index < self.shared.nodes.len()).then(|| SimLink {
            shared: Arc::clone(&self.shared),
            node: index,
        })
    }

    /// Reboots the `index`-th node, an access point that is up; `false` otherwise.
    pub fn reboot(&self, index: usize) -> bool {
        let mut world = self.shared.world();
        let mut happenings = Vec::new();
        let rebooted = world.reboot(index, &mut happenings);
        self.shared.dispatch(&world, &happenings);
        rebooted
    }
}

impl Drop for SimFleet {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

impl Shared {
    pub(crate) fn world(&self) -> MutexGuard<'_, World> {
        lock(&self.world)
    }

    pub(crate) fn session(&self, node: usize) -> MutexGuard<'_, Session> {
        lock(&self.nodes[node].session)
    }

    /// One second of the world, and what the agents push about it.
    pub(crate) fn step(&self) {
        let mut world = self.world();
        let happenings = world.step();
        self.dispatch(&world, &happenings);
        for node in 0..self.nodes.len() {
            self.connect(&world, node);
            if *lock(&self.nodes[node].connection) == Connection::Up {
                self.push_due(&world, node);
            }
        }
    }

    fn dispatch(&self, world: &World, happenings: &[Happening]) {
        for happening in happenings {
            let node = happening.node;
            if happening.what == What::Down {
                self.drop_link(world, node);
                continue;
            }
            let Some((topic, name, data)) = render::notification(world, happening) else {
                continue;
            };
            let up = *lock(&self.nodes[node].connection) == Connection::Up;
            if up && self.session(node).topics.contains(&topic) {
                self.push(
                    node,
                    AgentEvent::Event {
                        topic: name,
                        ts: Some(happening.at),
                        data: raw(&data),
                    },
                );
            }
        }
    }

    fn drop_link(&self, world: &World, node: usize) {
        *self.session(node) = Session::default();
        *lock(&self.nodes[node].connection) = Connection::Retrying {
            at: world.now_s + RETRY_MIN_S,
            attempt: 1,
        };
        self.nodes[node].state.send_replace(LinkState::Down {
            error: "connection lost: Connection reset by peer".to_owned(),
            retry_at: world.now().saturating_add(Duration::from_secs(RETRY_MIN_S)),
        });
    }

    /// Advances the connection to a node that is not up by one stage.
    fn connect(&self, world: &World, node: usize) {
        let mut connection = lock(&self.nodes[node].connection);
        let state = &self.nodes[node].state;
        *connection = match *connection {
            Connection::Retrying { at, attempt } if world.now_s >= at => {
                state.send_replace(LinkState::Connecting { attempt });
                Connection::Connecting { attempt }
            }
            Connection::Up | Connection::Retrying { .. } => return,
            Connection::Connecting { .. } if world.nodes[node].up() => {
                state.send_replace(LinkState::Up {
                    since: world.now(),
                    agent: AgentState::Starting,
                });
                Connection::Starting
            }
            Connection::Connecting { attempt } => {
                let wait = (RETRY_MIN_S << attempt.min(3)).min(RETRY_MAX_S);
                state.send_replace(LinkState::Down {
                    error: "cannot connect: No route to host".to_owned(),
                    retry_at: world.now().saturating_add(Duration::from_secs(wait)),
                });
                Connection::Retrying {
                    at: world.now_s + wait,
                    attempt: attempt + 1,
                }
            }
            Connection::Starting => {
                state.send_replace(running(world, node));
                Connection::Up
            }
        };
    }

    /// The ticks and the log batch that are due.
    fn push_due(&self, world: &World, node: usize) {
        let now_ms = world.now_ms();
        let mut pushes = Vec::new();
        {
            let mut session = self.session(node);
            for (&interval, tick) in &mut session.ticks {
                if tick.due_ms <= now_ms {
                    let sets: Vec<Set> = tick.sets.iter().copied().collect();
                    pushes.push(AgentEvent::Tick(raw(&render::collect(world, node, &sets))));
                    tick.due_ms = (tick.due_ms + interval as i64).max(now_ms + 1);
                }
            }
            if let Some(log) = &mut session.log
                && log.due_ms <= now_ms
            {
                log.due_ms = (log.due_ms + log.interval_ms as i64).max(now_ms + 1);
                let records = &world.nodes[node].logd.records;
                let fresh: Vec<_> = records.iter().filter(|r| r.id > log.after_id).collect();
                if let Some(last) = fresh.last() {
                    let first = fresh[0].id;
                    let dropped = first.saturating_sub(log.after_id + 1);
                    let batch = match log.mode {
                        LogMode::Stream => render::log_lines(&fresh),
                        LogMode::Poll | LogMode::Off => render::log_entries(&fresh, dropped),
                    };
                    log.after_id = last.id;
                    pushes.push(AgentEvent::Event {
                        topic: "log".to_owned(),
                        ts: Some(world.now()),
                        data: raw(&batch),
                    });
                }
            }
        }
        for push in pushes {
            self.push(node, push);
        }
    }

    fn push(&self, node: usize, event: AgentEvent) {
        // Nobody listening is fine.
        let _ = self.nodes[node].pushes.send(event);
    }
}

fn running(world: &World, node: usize) -> LinkState {
    LinkState::Up {
        since: world.now(),
        agent: AgentState::Running(render::hello(world, node)),
    }
}

fn raw(value: &serde_json::Value) -> Arc<serde_json::value::RawValue> {
    // A Value always encodes; the default (null) is never used.
    Arc::from(to_raw_value(value).unwrap_or_default())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
