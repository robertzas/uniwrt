//! The adoption port's watch (features/adoption.md, Path C; D-106): when the gateway's agent
//! reports a link on the site's adoption port (or its bridge, `br-adopt`), and when the gateway
//! comes online (a device may have been plugged in meanwhile), the jobs look for a
//! factory-default device there ([`Jobs::look_on_adoption_port`]). While nothing answers, they
//! look again every [`RETRY_EVERY`] for [`LOOK_FOR`]: a device brings its port up early in its
//! boot and its SSH server a minute or two later. Links of the port and its bridge come
//! together, so a look waits [`SETTLE`] after the last.

use std::time::Duration;

use tokio::{
    sync::{broadcast::error::RecvError, watch},
    task::JoinHandle,
    time::{Instant, sleep_until},
};
use tracing::{debug, warn};
use uniwrt_core::NodeId;
use uniwrt_engine::{Engine, Event};
use uniwrt_jobs::{Jobs, PortLook};
use uniwrt_model::{
    event::{DeviceChange, NodeEvent},
    node::NodeRole,
    status::NodeState,
};
use uniwrt_node::adoption_port::is_port_link;
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, Store};

use crate::fleet_link::FleetLink;

const SETTLE: Duration = Duration::from_secs(2);
const RETRY_EVERY: Duration = Duration::from_secs(10);
const LOOK_FOR: Duration = Duration::from_mins(3);

pub(crate) struct PortWatch {
    task: JoinHandle<()>,
    stop: watch::Sender<bool>,
}

impl PortWatch {
    pub(crate) fn start(store: Store, jobs: Jobs, engine: &Engine<FleetLink>) -> Self {
        let mut events = engine.subscribe();
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut plan: Option<Plan> = None;
            loop {
                let due = plan.map(|plan| plan.at);
                tokio::select! {
                    event = events.recv() => match event {
                        Ok(event) => {
                            if let Some(gateway) = trigger(&store, &event).await {
                                plan = Some(Plan::triggered(gateway, Instant::now(), plan));
                            }
                        }
                        Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => return,
                    },
                    () = sleep_until(due.unwrap_or_else(Instant::now)), if due.is_some() => {
                        if let Some(current) = plan.take() {
                            plan = look(&jobs, current).await;
                        }
                    }
                    _ = stopped.changed() => return,
                }
            }
        });
        Self { task, stop }
    }

    pub(crate) async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

/// The next look: at the gateway, when, and until when to keep looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    gateway: NodeId,
    at: Instant,
    until: Instant,
}

impl Plan {
    /// A link came up on the port, or the gateway came online, at `now`.
    fn triggered(gateway: NodeId, now: Instant, before: Option<Self>) -> Self {
        let at = now + SETTLE;
        Self {
            gateway,
            // A flapping link doesn't hold a look off.
            at: before
                .filter(|before| before.gateway == gateway)
                .map_or(at, |before| before.at.min(at)),
            until: now + LOOK_FOR,
        }
    }

    /// After a look that found nothing at `now`: the next, while still looking.
    fn again(self, now: Instant) -> Option<Self> {
        let at = now + RETRY_EVERY;
        (at <= self.until).then_some(Self { at, ..self })
    }
}

/// The gateway an event asks to look at the adoption port of, if any.
async fn trigger(store: &Store, event: &Event) -> Option<NodeId> {
    let (node, device) = match event {
        Event::Status { node, status } if status.state == NodeState::Online => (*node, None),
        Event::Node {
            node,
            event: NodeEvent::Device(device),
            ..
        } if device.change == DeviceChange::LinkUp => (*node, Some(device.name.as_str())),
        _ => return None,
    };
    let port = store.head_revision().await.ok()??.config.adoption_port?;
    if device.is_some_and(|device| !is_port_link(device, &port.port)) {
        return None;
    }
    let gateway = store.node(node).await.ok()??;
    (gateway.role == NodeRole::Gateway).then_some(node)
}

async fn look(jobs: &Jobs, plan: Plan) -> Option<Plan> {
    match jobs.look_on_adoption_port(plan.gateway).await {
        Ok(PortLook::Nothing) => plan.again(Instant::now()),
        Ok(look) => {
            debug!(?look, gateway = %plan.gateway, "looked on the adoption port");
            None
        }
        Err(err) => {
            warn!(%err, "cannot look on the adoption port");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uniwrt_core::{MacAddr, SystemClock, Timestamp};
    use uniwrt_model::{
        event::DeviceEvent,
        node::NewNode,
        site::{
            SiteConfig,
            adoption::AdoptionPort,
            revision::{NewRevision, RevisionKind},
        },
        status::NodeStatus,
    };

    use super::*;

    #[test]
    fn a_plan_settles_then_retries_for_a_while() {
        let gw = NodeId::new(1);
        let now = Instant::now();
        let plan = Plan::triggered(gw, now, None);
        assert_eq!((plan.at, plan.until), (now + SETTLE, now + LOOK_FOR));
        // The bridge's link a second later doesn't put the look off, but extends the search.
        let later = now + Duration::from_secs(1);
        let again = Plan::triggered(gw, later, Some(plan));
        assert_eq!((again.at, again.until), (plan.at, later + LOOK_FOR));
        assert_eq!(
            plan.again(now + Duration::from_mins(1)).unwrap().at,
            now + Duration::from_mins(1) + RETRY_EVERY
        );
        assert_eq!(plan.again(now + LOOK_FOR), None);
    }

    fn link(node: NodeId, name: &str, change: DeviceChange) -> Event {
        Event::Node {
            node,
            at: Timestamp::from_unix_secs(0),
            reported_at: None,
            event: NodeEvent::Device(DeviceEvent {
                name: name.to_owned(),
                change,
                present: true,
                active: true,
                carrier: true,
            }),
        }
    }

    fn status(node: NodeId, state: NodeState) -> Event {
        Event::Status {
            node,
            status: NodeStatus {
                state,
                since: Timestamp::from_unix_secs(0),
                reasons: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn only_the_gateway_s_port_links_and_its_coming_online_trigger_a_look() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("uniwrt.db"), Arc::new(SystemClock))
            .await
            .unwrap();
        let mut ids = Vec::new();
        for (last, role) in [(1, NodeRole::Gateway), (2, NodeRole::Ap)] {
            let node = NewNode {
                name: format!("n{last}"),
                mac: MacAddr::new([2, 0, 0, 0, 0, last]),
                board_name: "x".to_owned(),
                host: format!("10.0.0.{last}"),
                port: 22,
                role,
            };
            ids.push(store.create_node(node).await.unwrap().id);
        }
        let (gw, ap) = (ids[0], ids[1]);
        // No adoption port in the site yet.
        let up = link(gw, "lan4", DeviceChange::LinkUp);
        assert_eq!(trigger(&store, &up).await, None);
        let mut config = SiteConfig::defaults(None);
        config.adoption_port = Some(AdoptionPort::new("lan4"));
        let revision = NewRevision {
            base: None,
            kind: RevisionKind::Edit,
            restored_from: None,
            author_id: None,
            author: None,
            message: "adoption port".to_owned(),
            config,
        };
        store.commit_revision(revision, Vec::new()).await.unwrap();
        assert_eq!(trigger(&store, &up).await, Some(gw));
        let bridge = link(gw, "br-adopt", DeviceChange::LinkUp);
        assert_eq!(trigger(&store, &bridge).await, Some(gw));
        let online = status(gw, NodeState::Online);
        assert_eq!(trigger(&store, &online).await, Some(gw));
        for other in [
            link(gw, "lan3", DeviceChange::LinkUp),
            link(gw, "lan4", DeviceChange::LinkDown),
            link(ap, "lan4", DeviceChange::LinkUp),
            status(ap, NodeState::Online),
            status(gw, NodeState::Offline),
        ] {
            assert_eq!(trigger(&store, &other).await, None, "{other:?}");
        }
    }
}
