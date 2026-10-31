//! [`Engine`]: the fleet's node actors, the live state they keep and the bus they announce on.

use std::{
    any::Any,
    collections::BTreeMap,
    panic::{self, AssertUnwindSafe},
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
    time::Instant,
};
use tracing::{Instrument as _, error, info_span};
use uniwrt_core::{Clock, NodeId};
use uniwrt_model::{facts::NodeFacts, status::NodeStatus};
use uniwrt_node::NodeLink;
use uniwrt_topo::Topology;

use crate::{
    EngineConfig, Event, Interest, NodeView,
    actor::NodeActor,
    interest::{Interests, Kind},
    live::Live,
    roam::RoamTracker,
    status::{self, Health},
    topology::{self, Identity, TopologyState},
};

/// An actor that panicked starts again after this, doubling up to `RESTART_MAX`; one that ran
/// for `RESTART_STABLE` first starts the backoff over.
const RESTART_MIN: Duration = Duration::from_secs(1);
const RESTART_MAX: Duration = Duration::from_secs(60);
const RESTART_STABLE: Duration = Duration::from_secs(60);

/// What the actors share.
pub(crate) struct Context {
    pub(crate) config: EngineConfig,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) live: Live,
    bus: broadcast::Sender<Event>,
    roams: Mutex<RoamTracker>,
    pub(crate) topology: TopologyState,
}

impl Context {
    /// Announces an event; nobody listening is fine.
    pub(crate) fn publish(&self, event: Event) {
        if matches!(
            event,
            Event::Status { .. } | Event::Facts { .. } | Event::Removed { .. }
        ) {
            self.topology.changed();
        }
        let _ = self.bus.send(event);
    }

    pub(crate) fn roams(&self) -> MutexGuard<'_, RoamTracker> {
        self.roams.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The engine: one actor per watched node. Clones share it; dropping the last one stops the
/// actors.
pub struct Engine<L> {
    inner: Arc<Inner<L>>,
}

impl<L> Clone for Engine<L> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<L> {
    ctx: Arc<Context>,
    nodes: Mutex<BTreeMap<NodeId, Entry<L>>>,
    /// The topology's builder, started with the first node.
    builder: Mutex<Option<Task>>,
}

struct Entry<L> {
    link: Arc<L>,
    interests: Arc<watch::Sender<Interests>>,
    task: Task,
}

impl<L: NodeLink> Engine<L> {
    pub fn new(config: EngineConfig, clock: Arc<dyn Clock>) -> Self {
        let roams = Mutex::new(RoamTracker::new(config.roam_window));
        let ctx = Context {
            bus: broadcast::channel(config.bus_capacity).0,
            config,
            clock,
            live: Live::default(),
            roams,
            topology: TopologyState::default(),
        };
        Self {
            inner: Arc::new(Inner {
                ctx: Arc::new(ctx),
                nodes: Mutex::default(),
                builder: Mutex::default(),
            }),
        }
    }

    /// Watches a node over `link`, replacing an earlier link to it (and its actor). `last` is
    /// its latest stored snapshot, shown until fresh facts arrive; without one, the facts of
    /// the link replaced stay, as do the interests held in it. `identity` places it in the
    /// topology.
    pub async fn add(&self, node: NodeId, identity: Identity, link: L, last: Option<NodeFacts>) {
        let replaced = self.nodes().remove(&node);
        let interests = match replaced {
            Some(replaced) => {
                replaced.task.stop().await;
                replaced.interests
            }
            None => Arc::new(watch::channel(Interests::default()).0),
        };
        let ctx = &self.inner.ctx;
        let link = Arc::new(link);
        let facts = last
            .map(Arc::new)
            .or_else(|| ctx.live.get(node).and_then(|view| view.facts));
        let (state, reasons) = status::derive(&link.state().borrow(), &Health::default());
        let status = NodeStatus {
            state,
            since: ctx.clock.now(),
            reasons,
        };
        ctx.live.insert(node, NodeView { status, facts });
        ctx.topology.identify(node, identity);
        self.inner
            .builder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(|| Task(Some(tokio::spawn(topology::run(Arc::clone(ctx))))));
        let actor = supervise(
            Arc::clone(ctx),
            node,
            Arc::clone(&link),
            Arc::clone(&interests),
        )
        .instrument(info_span!("node", node_id = %node));
        let task = Task(Some(tokio::spawn(actor)));
        self.nodes().insert(
            node,
            Entry {
                link,
                interests,
                task,
            },
        );
    }

    /// Stops watching a node: its actor ends, its live state goes, and
    /// [`Event::Removed`] says so. False if it was not watched.
    pub async fn remove(&self, node: NodeId) -> bool {
        let Some(entry) = self.nodes().remove(&node) else {
            return false;
        };
        entry.task.stop().await;
        let ctx = &self.inner.ctx;
        ctx.live.remove(node);
        ctx.roams().forget(node);
        ctx.topology.forget(node);
        ctx.publish(Event::Removed { node });
        true
    }

    /// Stops every actor; the live state stays readable, for a last snapshot.
    pub async fn shutdown(&self) {
        let builder = self
            .inner
            .builder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(builder) = builder {
            builder.stop().await;
        }
        let nodes = std::mem::take(&mut *self.nodes());
        for entry in nodes.into_values() {
            entry.task.stop().await;
        }
    }

    /// The node's link, for requests of its own (jobs, diagnostics).
    pub fn link(&self, node: NodeId) -> Option<Arc<L>> {
        self.nodes().get(&node).map(|entry| Arc::clone(&entry.link))
    }

    /// Fast telemetry for the node while the result is held: the UI has it open
    /// ([`Intervals::focused`](crate::Intervals::focused)).
    pub fn focus(&self, node: NodeId) -> Option<Interest> {
        self.interest(node, Kind::Focus)
    }

    /// Follows the node's log while the result is held: [`Event::Log`]s.
    pub fn follow_logs(&self, node: NodeId) -> Option<Interest> {
        self.interest(node, Kind::Logs)
    }

    /// Events from now on. Read [`node`](Self::node) or [`all`](Self::all) after subscribing,
    /// so that nothing falls between.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.ctx.bus.subscribe()
    }

    /// The latest topology of the fleet, built a moment after its nodes changed
    /// ([`EngineConfig::topology_delay`]); [`Event::Topology`] announces each new one.
    pub fn topology(&self) -> Arc<Topology> {
        self.inner.ctx.topology.current()
    }

    pub fn node(&self, node: NodeId) -> Option<NodeView> {
        self.inner.ctx.live.get(node)
    }

    /// Every watched node, by id.
    pub fn all(&self) -> Vec<(NodeId, NodeView)> {
        self.inner.ctx.live.all()
    }

    pub(crate) fn context(&self) -> &Arc<Context> {
        &self.inner.ctx
    }

    /// How many hold interest in the node; `None` if it is not watched.
    pub fn interests(&self, node: NodeId) -> Option<Interests> {
        self.nodes()
            .get(&node)
            .map(|entry| *entry.interests.borrow())
    }

    fn interest(&self, node: NodeId, kind: Kind) -> Option<Interest> {
        let nodes = self.nodes();
        let entry = nodes.get(&node)?;
        Some(Interest::hold(&entry.interests, kind))
    }

    fn nodes(&self) -> MutexGuard<'_, BTreeMap<NodeId, Entry<L>>> {
        self.inner
            .nodes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs the node's actor, and starts it again with backoff when it panics: a bug in handling
/// one node must not stop watching it.
async fn supervise<L: NodeLink>(
    ctx: Arc<Context>,
    node: NodeId,
    link: Arc<L>,
    interests: Arc<watch::Sender<Interests>>,
) {
    let mut delay = RESTART_MIN;
    loop {
        let started = Instant::now();
        let actor = NodeActor::new(
            Arc::clone(&ctx),
            node,
            Arc::clone(&link),
            interests.subscribe(),
        );
        let Err(panic) = (CatchUnwind(Box::pin(actor.run()))).await else {
            return;
        };
        if started.elapsed() >= RESTART_STABLE {
            delay = RESTART_MIN;
        }
        error!(
            panic = panic_message(panic.as_ref()),
            "the node's actor panicked; restarting it in {delay:?}"
        );
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RESTART_MAX);
    }
}

/// A future that ends with its panic, if it panics.
struct CatchUnwind<F>(Pin<Box<F>>);

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, Box<dyn Any + Send>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        let inner = self.0.as_mut();
        match panic::catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(output)) => Poll::Ready(Ok(output)),
            Err(panic) => Poll::Ready(Err(panic)),
        }
    }
}

fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

/// A spawned actor, aborted when dropped.
struct Task(Option<JoinHandle<()>>);

impl Task {
    /// Aborts the actor and waits until it is gone, so it writes nothing after.
    async fn stop(mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}
