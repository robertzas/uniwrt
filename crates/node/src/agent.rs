//! The agent's RPC multiplexer: requests go to the agent's stdin with an id, and answers, events
//! and ticks come back on its stdout (see [`proto`](crate::proto)). Calls are cancel-safe and
//! time out on their own. When the agent goes away, every call in flight fails at once and later
//! calls fail fast.

use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde_json::value::RawValue;
use tokio::sync::{broadcast, oneshot, watch};
use tracing::{debug, warn};

use crate::{
    Error, Result,
    exec::{ExecStream, ExitStatus, Output, Outputs, Stdin},
    lines::{LineBuf, TooLong},
    proto::{AgentEvent, Hello, MAX_LINE, Message, RemoteError, Request, SUPPORTED},
};

/// How long a started agent may take to say hello.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// A running agent; clones share it. Dropping the last clone closes the agent's stdin, which
/// tells it to exit.
#[derive(Clone)]
pub struct Agent(Arc<Inner>);

struct Inner {
    stdin: Stdin,
    next_id: AtomicU64,
    calls: Arc<Calls>,
    hello: Hello,
    closed: watch::Receiver<Option<String>>,
}

impl Agent {
    /// Takes over the stream of a started agent: waits for its hello, checks the protocol
    /// version and starts dispatching. Events go to `events`, which may outlive the agent (a
    /// link keeps one channel across reconnects).
    pub async fn start(
        stream: ExecStream,
        events: broadcast::Sender<AgentEvent>,
        hello_timeout: Duration,
    ) -> Result<Self> {
        let (stdin, output) = stream.into_parts();
        let mut reader = Reader {
            output,
            lines: LineBuf::new(MAX_LINE),
            stderr: Tail::default(),
            status: None,
        };
        let hello = tokio::time::timeout(hello_timeout, reader.hello())
            .await
            .map_err(|_| Error::Timeout {
                what: "the agent's hello".to_owned(),
                after: hello_timeout,
            })??;
        if !SUPPORTED.contains(&hello.proto) {
            return Err(Error::Protocol(format!(
                "the agent speaks protocol {}, this controller {}..={}",
                hello.proto,
                SUPPORTED.start(),
                SUPPORTED.end()
            )));
        }
        let calls = Arc::new(Calls::default());
        let (closed_tx, closed) = watch::channel(None);
        tokio::spawn(reader.run(Arc::clone(&calls), events, closed_tx));
        Ok(Self(Arc::new(Inner {
            stdin,
            next_id: AtomicU64::new(1),
            calls,
            hello,
            closed,
        })))
    }

    pub fn hello(&self) -> &Hello {
        &self.0.hello
    }

    /// Sends one request and waits for its answer. `args` is the op's argument object.
    pub async fn call(
        &self,
        op: &str,
        args: &RawValue,
        timeout: Duration,
    ) -> Result<Box<RawValue>> {
        let id = self.0.next_id.fetch_add(1, Ordering::Relaxed);
        let answer = self
            .0
            .calls
            .register(id)
            .map_err(|reason| Error::AgentUnavailable { reason })?;
        // Whatever ends this call (an answer, the timeout, the caller dropping it) unregisters it.
        let _forget = Forget(&self.0.calls, id);
        let mut line = serde_json::to_vec(&Request { id, op, args })
            .map_err(|err| Error::Protocol(format!("cannot encode op {op}: {err}")))?;
        line.push(b'\n');
        let exchange = async {
            self.0.stdin.write(line).await?;
            answer.await.map_err(|_| Error::AgentUnavailable {
                reason: "the agent stopped".to_owned(),
            })
        };
        let answer = tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| Error::Timeout {
                what: format!("agent op {op}"),
                after: timeout,
            })??;
        answer.map_err(|failure| match failure {
            Failure::Remote(RemoteError { code, msg }) => Error::Remote {
                op: op.to_owned(),
                code,
                msg,
            },
            Failure::Closed(reason) => Error::AgentUnavailable { reason },
        })
    }

    pub fn is_closed(&self) -> bool {
        self.0.closed.borrow().is_some()
    }

    /// Waits until the agent has gone away, and says why.
    pub async fn closed(&self) -> String {
        let mut closed = self.0.closed.clone();
        let _ = closed.wait_for(Option::is_some).await;
        closed
            .borrow()
            .clone()
            .unwrap_or_else(|| "the agent stopped".to_owned())
    }
}

impl fmt::Debug for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agent")
            .field("hello", &self.0.hello)
            .finish_non_exhaustive()
    }
}

/// Why a call got no answer.
enum Failure {
    Remote(RemoteError),
    Closed(String),
}

type Answer = std::result::Result<Box<RawValue>, Failure>;

/// The calls waiting for an answer, by id.
#[derive(Default)]
struct Calls(Mutex<CallTable>);

#[derive(Default)]
struct CallTable {
    waiting: HashMap<u64, oneshot::Sender<Answer>>,
    /// Why the agent went away; set once, after which nothing registers.
    closed: Option<String>,
}

impl Calls {
    fn table(&self) -> MutexGuard<'_, CallTable> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn register(&self, id: u64) -> std::result::Result<oneshot::Receiver<Answer>, String> {
        let mut table = self.table();
        if let Some(reason) = &table.closed {
            return Err(reason.clone());
        }
        let (sender, receiver) = oneshot::channel();
        table.waiting.insert(id, sender);
        Ok(receiver)
    }

    fn answer(&self, id: u64, answer: Answer) {
        let waiting = self.table().waiting.remove(&id);
        if let Some(waiting) = waiting {
            let _ = waiting.send(answer);
        } else {
            debug!(id, "dropped an answer nobody waits for (timed out?)");
        }
    }

    fn close(&self, reason: &str) {
        let mut table = self.table();
        table.closed = Some(reason.to_owned());
        for (_, waiting) in table.waiting.drain() {
            let _ = waiting.send(Err(Failure::Closed(reason.to_owned())));
        }
    }
}

struct Forget<'a>(&'a Calls, u64);

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        self.0.table().waiting.remove(&self.1);
    }
}

/// Reads the agent's output: the hello first, then everything else until the stream ends.
struct Reader {
    output: Outputs,
    lines: LineBuf,
    stderr: Tail,
    status: Option<ExitStatus>,
}

impl Reader {
    async fn hello(&mut self) -> Result<Hello> {
        loop {
            let line = self.lines.next_line().map_err(|TooLong| too_long())?;
            if let Some(line) = line {
                return match Message::parse(line) {
                    Ok(Message::Hello(hello)) => Ok(hello),
                    Ok(_) => Err(Error::Protocol(
                        "the agent's first line is not a hello".to_owned(),
                    )),
                    Err(err) => Err(Error::Protocol(format!("the agent's first line: {err}"))),
                };
            }
            match self.read().await {
                Ok(true) => {}
                Ok(false) => {
                    return Err(Error::AgentUnavailable {
                        reason: self.ended(),
                    });
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn run(
        mut self,
        calls: Arc<Calls>,
        events: broadcast::Sender<AgentEvent>,
        closed: watch::Sender<Option<String>>,
    ) {
        let reason = self.dispatch(&calls, &events).await;
        debug!(%reason, "the agent went away");
        calls.close(&reason);
        closed.send_replace(Some(reason));
    }

    /// Dispatches lines until the stream ends; returns why it ended.
    async fn dispatch(&mut self, calls: &Calls, events: &broadcast::Sender<AgentEvent>) -> String {
        loop {
            loop {
                let line = match self.lines.next_line() {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(TooLong) => return too_long().to_string(),
                };
                match Message::parse(line) {
                    Ok(Message::Answer { id, result }) => {
                        calls.answer(id, result.map_err(Failure::Remote));
                    }
                    // Nobody listening is fine: events are not queued for later.
                    Ok(Message::Event(event)) => drop(events.send(event)),
                    Ok(Message::Hello(_)) => warn!("ignored a second hello from the agent"),
                    Err(err) => warn!(%err, "ignored a malformed line from the agent"),
                }
            }
            match self.read().await {
                Ok(true) => {}
                Ok(false) => return self.ended(),
                Err(err) => return err.to_string(),
            }
        }
    }

    /// Takes the next output into the line buffer or the stderr tail; false at the end.
    async fn read(&mut self) -> Result<bool> {
        match self.output.next().await {
            Some(Ok(Output::Stdout(data))) => self.lines.push(&data),
            Some(Ok(Output::Stderr(data))) => {
                debug!(stderr = %String::from_utf8_lossy(&data).trim_end(), "agent");
                self.stderr.push(&data);
            }
            Some(Ok(Output::Exit(status))) => self.status = Some(status),
            Some(Err(err)) => return Err(err),
            None => return Ok(false),
        }
        Ok(true)
    }

    fn ended(&self) -> String {
        let status = match &self.status {
            Some(status) => format!("the agent exited with {status}"),
            None => "the agent's channel closed".to_owned(),
        };
        match self.stderr.text() {
            stderr if stderr.is_empty() => status,
            stderr => format!("{status}: {stderr}"),
        }
    }
}

fn too_long() -> Error {
    Error::Protocol(format!(
        "the agent sent a line longer than {MAX_LINE} bytes"
    ))
}

/// The end of the agent's stderr, to explain why it went away (`ucode: not found`).
#[derive(Default)]
struct Tail(Vec<u8>);

impl Tail {
    const MAX: usize = 1024;

    fn push(&mut self, data: &[u8]) {
        self.0.extend_from_slice(data);
        if self.0.len() > Self::MAX {
            self.0.drain(..self.0.len() - Self::MAX);
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0).trim().to_owned()
    }
}

#[cfg(test)]
mod tests;
