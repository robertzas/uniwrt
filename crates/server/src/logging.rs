//! Logging: `tracing` to stderr, filtered by `--log` / `UNIWRT_LOG` (`EnvFilter` syntax).

use std::io::{self, IsTerminal as _};

use anyhow::{Context as _, anyhow};
use tracing_subscriber::EnvFilter;

pub(crate) fn init(filter: &str) -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_new(filter).with_context(|| format!("invalid log filter {filter:?}"))?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .try_init()
        .map_err(|err| anyhow!("cannot install the logger: {err}"))
}
