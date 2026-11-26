//! `serve --profile-cpu <FILE>` (the `profiling` feature; `just profile serve`, D-149): the
//! whole process, every thread, sampled from the start of `serve` to its exit, and the
//! flamegraph (SVG) written to FILE, as the benches are (`uniwrt-bench`: pprof in process, no
//! perf, no root, user space only). The feature is off in every release build; the `profiling`
//! cargo profile is the release build with its symbols kept, so the graph names functions.

use std::path::PathBuf;

use anyhow::{Context as _, anyhow};
use pprof::ProfilerGuard;
use tracing::info;

/// A running profile, written by [`Profile::finish`].
pub(crate) struct Profile {
    guard: ProfilerGuard<'static>,
    path: PathBuf,
}

/// Starts sampling when `path` names a file to write the flamegraph to.
pub(crate) fn start(path: Option<PathBuf>) -> anyhow::Result<Option<Profile>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let guard = uniwrt_bench::sample()
        .map_err(|err| anyhow!(err))
        .context("cannot start the CPU profiler")?;
    info!(path = %path.display(), "profiling the CPU until exit");
    Ok(Some(Profile { guard, path }))
}

impl Profile {
    /// Writes the flamegraph of everything sampled since the start.
    pub(crate) fn finish(self) -> anyhow::Result<()> {
        uniwrt_bench::write_flamegraph(&self.guard, &self.path)
            .map_err(|err| anyhow!(err))
            .with_context(|| format!("cannot write the flamegraph to {}", self.path.display()))?;
        info!(path = %self.path.display(), "CPU profile written");
        Ok(())
    }
}
