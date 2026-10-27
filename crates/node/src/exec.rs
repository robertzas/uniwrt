//! Running commands on a node: one-shot [`ExecRequest`]s and long-lived [`ExecStream`]s.

mod stream;

use std::{fmt, time::Duration};

use bytes::Bytes;

pub use self::stream::{
    ExecStream, Output, OutputSink, Outputs, Stdin, StdinSource, StreamSource, pipe,
};
use crate::{Error, Result};

/// How long a one-shot command may run unless the request says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// How much stdout and stderr (together) a one-shot command may return.
pub const DEFAULT_MAX_OUTPUT: usize = 16 << 20;
/// How much unread output a stream may queue before it is cut off.
pub const STREAM_BUDGET: usize = 4 << 20;

/// A command to run once, collecting its output.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub argv: Vec<String>,
    /// Written to the command's stdin, which is then closed. Without it, stdin is closed at once.
    pub stdin: Option<Bytes>,
    pub timeout: Duration,
    /// More stdout and stderr than this fails the request with [`Error::Overflow`].
    pub max_output: usize,
}

impl ExecRequest {
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            stdin: None,
            timeout: DEFAULT_TIMEOUT,
            max_output: DEFAULT_MAX_OUTPUT,
        }
    }

    #[must_use]
    pub fn stdin(mut self, data: impl Into<Bytes>) -> Self {
        self.stdin = Some(data.into());
        self
    }

    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn max_output(mut self, bytes: usize) -> Self {
        self.max_output = bytes;
        self
    }

    /// How errors and logs name this command: its first word.
    pub(crate) fn what(&self) -> String {
        what(&self.argv)
    }
}

pub(crate) fn what(argv: &[String]) -> String {
    format!("`{}`", argv.first().map_or("", String::as_str))
}

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitStatus {
    Code(u32),
    /// Killed by a signal, such as `KILL`.
    Signal(String),
    /// The channel closed without saying.
    Unknown,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        *self == Self::Code(0)
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code(code) => write!(f, "exit code {code}"),
            Self::Signal(signal) => write!(f, "signal {signal}"),
            Self::Unknown => f.write_str("no exit status"),
        }
    }
}

/// The result of a one-shot command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecOutput {
    /// The output if the command succeeded, else [`Error::Failed`] with the start of stderr.
    pub fn check(self, what: &str) -> Result<Self> {
        if self.status.success() {
            return Ok(self);
        }
        let stderr = String::from_utf8_lossy(&self.stderr);
        Err(Error::Failed {
            what: what.to_owned(),
            status: self.status,
            stderr: truncate(stderr.trim_end(), 512).to_owned(),
        })
    }
}

fn truncate(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Runs a one-shot command: opens its stream, then feeds stdin while collecting output (a
/// command may write before it has read all its input), all within the request's timeout and
/// output limit.
pub(crate) async fn collect(
    request: &ExecRequest,
    open: impl Future<Output = Result<ExecStream>>,
) -> Result<ExecOutput> {
    let what = request.what();
    let run = async {
        let (stdin, mut output) = open.await?.into_parts();
        let feed = async move {
            if let Some(data) = request.stdin.clone() {
                // A command that exits without reading its input is not an error here.
                let _ = stdin.write(data).await;
            }
        };
        let read = async {
            let mut result = ExecOutput {
                status: ExitStatus::Unknown,
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            while let Some(item) = output.next().await {
                match item? {
                    Output::Stdout(data) => result.stdout.extend_from_slice(&data),
                    Output::Stderr(data) => result.stderr.extend_from_slice(&data),
                    Output::Exit(status) => result.status = status,
                }
                if result.stdout.len() + result.stderr.len() > request.max_output {
                    return Err(Error::Overflow {
                        what: what.clone(),
                        limit: request.max_output,
                    });
                }
            }
            Ok(result)
        };
        tokio::join!(feed, read).1
    };
    tokio::time::timeout(request.timeout, run)
        .await
        .map_err(|_| Error::Timeout {
            what: what.clone(),
            after: request.timeout,
        })?
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;

    use super::{ExecOutput, ExecRequest, ExitStatus, Output, collect, pipe};
    use crate::Error;

    #[tokio::test]
    async fn collect_feeds_stdin_and_gathers_output() {
        let (stream, mut source) = pipe("`cat`", 1 << 20);
        let producer = tokio::spawn(async move {
            while let Some(data) = source.stdin.next().await {
                source.output.push(Output::Stdout(data));
            }
            source
                .output
                .push(Output::Stderr(Bytes::from_static(b"done")));
            source.output.push(Output::Exit(ExitStatus::Code(0)));
        });
        let request = ExecRequest::new(["cat"]).stdin("hello");
        let output = collect(&request, async { Ok(stream) }).await.unwrap();
        producer.await.unwrap();
        assert_eq!(
            output,
            ExecOutput {
                status: ExitStatus::Code(0),
                stdout: b"hello".to_vec(),
                stderr: b"done".to_vec(),
            }
        );
    }

    #[tokio::test]
    async fn collect_enforces_the_output_limit_and_the_timeout() {
        let (stream, source) = pipe("`yes`", 1 << 20);
        source
            .output
            .push(Output::Stdout(Bytes::from(vec![b'y'; 11])));
        let err = collect(&ExecRequest::new(["yes"]).max_output(10), async {
            Ok(stream)
        })
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Overflow { limit: 10, .. }), "{err:?}");

        let (stream, _source) = pipe("`sleep`", 1 << 20);
        let request = ExecRequest::new(["sleep"]).timeout(Duration::from_millis(20));
        let err = collect(&request, async { Ok(stream) }).await.unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    }

    #[test]
    fn check_reports_the_status_and_the_start_of_stderr() {
        let output = ExecOutput {
            status: ExitStatus::Code(2),
            stdout: Vec::new(),
            stderr: "é".repeat(400).into_bytes(),
        };
        let Err(Error::Failed { status, stderr, .. }) = output.check("`ls`") else {
            panic!("expected a failure");
        };
        assert_eq!(status, ExitStatus::Code(2));
        assert_eq!(stderr.len(), 512);
        assert_eq!(ExitStatus::Signal("KILL".into()).to_string(), "signal KILL");
    }
}
