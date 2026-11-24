//! Logging: `tracing` to stderr, filtered by `--log` / `UNIWRT_LOG` (`EnvFilter` syntax), and
//! the same lines, without colors, into the controller's log tail ([`tail`]) for the support
//! bundle (D-144).

use std::{
    io::{self, IsTerminal as _},
    sync::OnceLock,
};

use anyhow::{Context as _, anyhow};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use uniwrt_api::LogTail;

static TAIL: OnceLock<LogTail> = OnceLock::new();

/// The log tail, once [`init`] installed the logger.
pub(crate) fn tail() -> Option<LogTail> {
    TAIL.get().cloned()
}

/// Writes into the tail.
struct TailWriter(LogTail);

impl io::Write for TailWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn init(filter: &str) -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_new(filter).with_context(|| format!("invalid log filter {filter:?}"))?;
    let tail = LogTail::new();
    let ring = tail.clone();
    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(io::stderr)
                .with_ansi(io::stderr().is_terminal()),
        )
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(move || TailWriter(ring.clone())),
        )
        .try_init()
        .map_err(|err| anyhow!("cannot install the logger: {err}"))?;
    let _ = TAIL.set(tail);
    Ok(())
}
