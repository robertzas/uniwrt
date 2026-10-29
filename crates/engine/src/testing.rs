//! A scripted [`NodeLink`] for actor tests: the test sets its state, pushes its agent's events
//! and sees each request; collects are answered from the hwsim lab's capture of its gateway.

use std::{
    sync::{Arc, LazyLock, Mutex, PoisonError},
    time::Duration,
};

use bytes::Bytes;
use serde_json::{Value, json, value::RawValue};
use tokio::sync::{broadcast, mpsc, watch};
use uniwrt_core::Timestamp;
use uniwrt_node::{
    AgentEvent, AgentState, Error, ExecOutput, ExecRequest, ExecStream, Hello, LinkState, NodeLink,
    Result,
    proto::{
        AgentCaps,
        ops::{Set, Topic},
    },
};

/// The lab gateway's collect answer, every set.
static CAPTURE: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../node/fixtures/collect/hwsim-a-25.12.6.json"
    ))
    .expect("the capture is JSON")
});

/// One request the link was sent.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Call {
    pub(crate) op: String,
    pub(crate) args: Value,
}

type Answer = dyn Fn(&str, &Value) -> Option<Result<Value>> + Send + Sync;

pub(crate) struct FakeLink {
    state: watch::Sender<LinkState>,
    pushes: broadcast::Sender<AgentEvent>,
    calls: mpsc::UnboundedSender<Call>,
    answer: Mutex<Option<Arc<Answer>>>,
}

/// The test's side of a [`FakeLink`].
pub(crate) struct Script {
    pub(crate) link: Arc<FakeLink>,
    calls: mpsc::UnboundedReceiver<Call>,
}

/// A link to hand an engine, and the script that drives it.
pub(crate) fn link() -> (SharedLink, Script) {
    let (calls, receiver) = mpsc::unbounded_channel();
    let link = Arc::new(FakeLink {
        state: watch::channel(LinkState::Connecting { attempt: 1 }).0,
        pushes: broadcast::channel(64).0,
        calls,
        answer: Mutex::new(None),
    });
    (
        SharedLink(Arc::clone(&link)),
        Script {
            link,
            calls: receiver,
        },
    )
}

/// The agent's hello: every set and, with `events`, every topic.
pub(crate) fn hello(events: bool) -> Hello {
    Hello {
        proto: 1,
        agent: "test".to_owned(),
        release: Some("25.12.6".to_owned()),
        caps: AgentCaps {
            events,
            ops: vec![],
            sets: Set::ALL.iter().map(|set| set.as_str().to_owned()).collect(),
            topics: Topic::ALL
                .iter()
                .map(|topic| topic.as_str().to_owned())
                .collect(),
        },
    }
}

pub(crate) fn running(hello: Hello) -> LinkState {
    LinkState::Up {
        since: Timestamp::UNIX_EPOCH,
        agent: AgentState::Running(hello),
    }
}

/// The captured sources of `sets`, as a collect answer or a tick carries them.
pub(crate) fn collected(sets: &[Set]) -> Value {
    let captured = &CAPTURE["sets"];
    let sets: serde_json::Map<_, _> = sets
        .iter()
        .map(|set| (set.as_str().to_owned(), captured[set.as_str()].clone()))
        .collect();
    json!({"sets": sets, "errors": {}})
}

pub(crate) fn raw(value: &Value) -> Arc<RawValue> {
    serde_json::value::to_raw_value(value).unwrap().into()
}

impl Script {
    pub(crate) fn set_state(&self, state: LinkState) {
        self.link.state.send_replace(state);
    }

    /// Pushes an event to whoever listens (nobody, once the actor is gone).
    pub(crate) fn push(&self, event: AgentEvent) {
        let _ = self.link.pushes.send(event);
    }

    pub(crate) fn push_event(&self, topic: &str, data: &Value) {
        self.push(AgentEvent::Event {
            topic: topic.to_owned(),
            ts: Some(Timestamp::from_unix_ms(1_793_245_539_282)),
            data: raw(data),
        });
    }

    pub(crate) fn push_tick(&self, tick: &Value) {
        self.push(AgentEvent::Tick(raw(tick)));
    }

    /// Answers requests with `answer` where it returns `Some`, as an agent would otherwise.
    pub(crate) fn answer(
        &self,
        answer: impl Fn(&str, &Value) -> Option<Result<Value>> + Send + Sync + 'static,
    ) {
        *lock(&self.link.answer) = Some(Arc::new(answer));
    }

    /// The next request, within a minute of the paused clock.
    pub(crate) async fn call(&mut self) -> Call {
        tokio::time::timeout(Duration::from_secs(60), self.calls.recv())
            .await
            .expect("a request within a minute")
            .expect("the link is alive")
    }

    /// The next request of `op`, skipping others.
    pub(crate) async fn call_of(&mut self, op: &str) -> Call {
        loop {
            let call = self.call().await;
            if call.op == op {
                return call;
            }
        }
    }

    /// No request within `wait`.
    pub(crate) async fn no_call(&mut self, wait: Duration) {
        if let Ok(call) = tokio::time::timeout(wait, self.calls.recv()).await {
            panic!("unexpected request {call:?}");
        }
    }
}

/// How the agent answers: collects from the capture, the push ops as it would.
fn agent(op: &str, args: &Value) -> Result<Value> {
    Ok(match op {
        "collect" => {
            let sets: Vec<Set> = serde_json::from_value(args["sets"].clone()).unwrap();
            collected(&sets)
        }
        "subscribe" | "unsubscribe" => json!({"topics": args["topics"]}),
        "set_interval" => json!({"intervals": {}}),
        "log_follow" => json!({"mode": args["mode"]}),
        "ping" => json!({"ts": 0}),
        _ => {
            return Err(Error::Remote {
                op: op.to_owned(),
                code: "bad_request".to_owned(),
                msg: "unknown op".to_owned(),
            });
        }
    })
}

/// A [`FakeLink`] as the engine holds it.
pub(crate) struct SharedLink(Arc<FakeLink>);

impl NodeLink for SharedLink {
    fn call(
        &self,
        op: &str,
        args: &RawValue,
        _timeout: Duration,
    ) -> impl Future<Output = Result<Box<RawValue>>> + Send {
        let link = &self.0;
        let args: Value = serde_json::from_str(args.get()).unwrap();
        let _ = link.calls.send(Call {
            op: op.to_owned(),
            args: args.clone(),
        });
        let custom = lock(&link.answer).clone();
        let answer = custom
            .and_then(|answer| answer(op, &args))
            .unwrap_or_else(|| agent(op, &args));
        std::future::ready(answer.map(|answer| serde_json::value::to_raw_value(&answer).unwrap()))
    }

    fn exec(&self, _request: ExecRequest) -> impl Future<Output = Result<ExecOutput>> + Send {
        std::future::ready(Err(down()))
    }

    fn stream(&self, _argv: Vec<String>) -> impl Future<Output = Result<ExecStream>> + Send {
        std::future::ready(Err(down()))
    }

    fn upload(
        &self,
        _path: &str,
        _data: Bytes,
        _mode: u32,
    ) -> impl Future<Output = Result<()>> + Send {
        std::future::ready(Err(down()))
    }

    fn events(&self) -> broadcast::Receiver<AgentEvent> {
        self.0.pushes.subscribe()
    }

    fn state(&self) -> watch::Receiver<LinkState> {
        self.0.state.subscribe()
    }
}

fn down() -> Error {
    Error::Disconnected {
        what: "the fake link".to_owned(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
