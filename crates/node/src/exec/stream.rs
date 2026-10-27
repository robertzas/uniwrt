//! Long-lived commands: an [`ExecStream`] and its producer's end, a pair of queues.
//!
//! The producer (the SSH channel pump, or a simulator) never waits for the consumer: unread
//! output is counted against a byte budget, and a consumer that falls behind by more than the
//! budget gets [`Error::Overflow`] instead of stalling the connection that carries the stream.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use bytes::Bytes;
use tokio::sync::mpsc;

use super::ExitStatus;
use crate::{Error, Result};

/// Chunks of stdin queued ahead of the connection's flow control.
const STDIN_QUEUE: usize = 16;

/// One event of a running command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Stdout(Bytes),
    Stderr(Bytes),
    /// The command ended; the stream ends after this.
    Exit(ExitStatus),
}

impl Output {
    fn len(&self) -> usize {
        match self {
            Self::Stdout(data) | Self::Stderr(data) => data.len(),
            Self::Exit(_) => 0,
        }
    }
}

/// A running command: write to its stdin, read its output.
#[derive(Debug)]
pub struct ExecStream {
    stdin: Stdin,
    output: Outputs,
}

impl ExecStream {
    pub async fn write(&self, data: impl Into<Bytes>) -> Result<()> {
        self.stdin.write(data).await
    }

    /// Closes stdin: the command reads end of file.
    pub fn close_stdin(&mut self) {
        self.stdin.0 = None;
    }

    /// The next output, `None` once the stream has ended. An `Err` ends the stream too.
    pub async fn next(&mut self) -> Option<Result<Output>> {
        self.output.next().await
    }

    pub fn into_parts(self) -> (Stdin, Outputs) {
        (self.stdin, self.output)
    }
}

/// The writing half of an [`ExecStream`]; clones share one stdin, which closes when all drop.
#[derive(Debug, Clone)]
pub struct Stdin(Option<mpsc::Sender<Bytes>>, Arc<str>);

impl Stdin {
    pub async fn write(&self, data: impl Into<Bytes>) -> Result<()> {
        let disconnected = || Error::Disconnected {
            what: format!("stdin of {}", self.1),
        };
        let sender = self.0.as_ref().ok_or_else(disconnected)?;
        sender.send(data.into()).await.map_err(|_| disconnected())
    }
}

/// The reading half of an [`ExecStream`].
#[derive(Debug)]
pub struct Outputs {
    receiver: mpsc::UnboundedReceiver<Result<Output>>,
    queued: Arc<AtomicUsize>,
}

impl Outputs {
    pub async fn next(&mut self) -> Option<Result<Output>> {
        let item = self.receiver.recv().await?;
        if let Ok(output) = &item {
            self.queued.fetch_sub(output.len(), Ordering::Relaxed);
        }
        Some(item)
    }
}

/// The producer's end of a stream: where the command's stdin arrives and its output goes.
#[derive(Debug)]
pub struct StreamSource {
    pub stdin: StdinSource,
    pub output: OutputSink,
}

/// Chunks written to the command's stdin; `None` once stdin is closed.
#[derive(Debug)]
pub struct StdinSource(mpsc::Receiver<Bytes>);

impl StdinSource {
    pub async fn next(&mut self) -> Option<Bytes> {
        self.0.recv().await
    }
}

/// Where the producer puts output. It never waits; see [`OutputSink::push`].
#[derive(Debug)]
pub struct OutputSink {
    sender: mpsc::UnboundedSender<Result<Output>>,
    queued: Arc<AtomicUsize>,
    budget: usize,
    what: Arc<str>,
}

impl OutputSink {
    /// Queues an output. False means stop producing: the consumer is gone, or it fell more than
    /// the budget behind, in which case it reads [`Error::Overflow`] after what it has queued.
    pub fn push(&self, output: Output) -> bool {
        let queued = self.queued.fetch_add(output.len(), Ordering::Relaxed) + output.len();
        if queued > self.budget {
            self.fail(Error::Overflow {
                what: self.what.to_string(),
                limit: self.budget,
            });
            return false;
        }
        self.sender.send(Ok(output)).is_ok()
    }

    /// Ends the stream with an error.
    pub fn fail(&self, error: Error) {
        let _ = self.sender.send(Err(error));
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

/// A new stream and its producer's end. `what` names the command in errors; `budget` bounds the
/// unread output.
pub fn pipe(what: impl Into<Arc<str>>, budget: usize) -> (ExecStream, StreamSource) {
    let what = what.into();
    let (stdin_tx, stdin_rx) = mpsc::channel(STDIN_QUEUE);
    let (output_tx, output_rx) = mpsc::unbounded_channel();
    let queued = Arc::new(AtomicUsize::new(0));
    let stream = ExecStream {
        stdin: Stdin(Some(stdin_tx), Arc::clone(&what)),
        output: Outputs {
            receiver: output_rx,
            queued: Arc::clone(&queued),
        },
    };
    let source = StreamSource {
        stdin: StdinSource(stdin_rx),
        output: OutputSink {
            sender: output_tx,
            queued,
            budget,
            what,
        },
    };
    (stream, source)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::{Output, pipe};
    use crate::Error;

    #[tokio::test]
    async fn output_beyond_the_budget_ends_the_stream_without_blocking_the_producer() {
        let (mut stream, source) = pipe("`cat`", 10);
        assert!(
            source
                .output
                .push(Output::Stdout(Bytes::from_static(b"12345")))
        );
        assert!(
            source
                .output
                .push(Output::Stderr(Bytes::from_static(b"12345")))
        );
        assert!(!source.output.push(Output::Stdout(Bytes::from_static(b"x"))));
        assert!(matches!(stream.next().await, Some(Ok(Output::Stdout(_)))));
        assert!(matches!(stream.next().await, Some(Ok(Output::Stderr(_)))));
        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err, Error::Overflow { limit: 10, .. }), "{err:?}");
    }

    #[tokio::test]
    async fn reading_frees_budget() {
        let (mut stream, source) = pipe("`cat`", 4);
        for _ in 0..10 {
            assert!(
                source
                    .output
                    .push(Output::Stdout(Bytes::from_static(b"abcd")))
            );
            assert!(stream.next().await.unwrap().is_ok());
        }
    }

    #[tokio::test]
    async fn a_dropped_consumer_stops_the_producer_and_closes_its_stdin() {
        let (stream, mut source) = pipe("`cat`", 4);
        drop(stream);
        assert!(!source.output.push(Output::Stdout(Bytes::from_static(b"a"))));
        assert!(source.output.is_closed());
        assert_eq!(source.stdin.next().await, None);
    }
}
