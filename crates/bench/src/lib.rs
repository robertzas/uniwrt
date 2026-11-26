//! Benchmark support (docs/dev/TESTING.md, Performance; D-149): what the crates' criterion
//! benches share, and the CPU sampler behind every flamegraph.
//!
//! - **API:** [`criterion`] is the benches' configuration: criterion's defaults with
//!   [`Flamegraph`] as its profiler, which samples a bench while criterion runs it under
//!   `--profile-time <s>` and writes `target/criterion/<group>/<bench>/profile/flamegraph.svg`
//!   (`just profile bench [filter]`). [`sample`] starts the sampler and [`write_flamegraph`]
//!   writes what it took; the server's `profiling` feature uses both (`serve --profile-cpu`).
//! - **Benches:** `crates/engine/benches/ingest.rs` (the telemetry ingest on simulated fleets),
//!   `crates/render/benches/render.rs` (one node's packages) and `crates/syslog/benches/logs.rs`
//!   (parsing and annotating log lines). `cargo bench -p <crate> --bench <name>` measures;
//!   `cargo test` on the same targets runs each bench once, as CI does (no timing gate: shared
//!   runners vary by ±15 %).
//! - **Invariants:** a dev-dependency, and the `profiling` feature's: nothing here reaches a
//!   release build. pprof samples in process (a `SIGPROF` timer, every thread), so it needs
//!   neither perf nor root, and sees user space only; it never unwinds inside libc, libgcc,
//!   pthread or the vDSO, where unwinding from a signal can deadlock.
//! - **Dependencies:** criterion, pprof, inferno (the SVG; CDDL-1.0, deny.toml's exception).

use std::{fs::File, io::BufWriter, path::Path};

use criterion::{Criterion, profiler::Profiler};
use pprof::{ProfilerGuard, ProfilerGuardBuilder, Report};

/// Samples per second: a prime, so that sampling doesn't run in step with periodic work.
const FREQUENCY: i32 = 997;
const BLOCKLIST: [&str; 4] = ["libc", "libgcc", "pthread", "vdso"];

/// A failure to sample or to write the graph.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// The benches' configuration: criterion's defaults, profiled by [`Flamegraph`].
pub fn criterion() -> Criterion {
    Criterion::default().with_profiler(Flamegraph::default())
}

/// Starts sampling the process's CPU; it stops when the guard drops.
pub fn sample() -> Result<ProfilerGuard<'static>, Error> {
    Ok(ProfilerGuardBuilder::default()
        .frequency(FREQUENCY)
        .blocklist(&BLOCKLIST)
        .build()?)
}

/// Writes what `guard` sampled so far as a flamegraph (SVG) at `path`, creating its directory.
pub fn write_flamegraph(guard: &ProfilerGuard<'_>, path: &Path) -> Result<(), Error> {
    let report = guard.report().build()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let lines = folded(&report);
    let mut options = inferno::flamegraph::Options::default();
    let out = BufWriter::new(File::create(path)?);
    inferno::flamegraph::from_lines(&mut options, lines.iter().map(String::as_str), out)?;
    Ok(())
}

/// The report's stacks in the folded format inferno reads: "thread;outer;…;inner count", a
/// line per distinct stack, sorted.
fn folded(report: &Report) -> Vec<String> {
    let mut lines: Vec<String> = report
        .data
        .iter()
        .map(|(stack, count)| {
            let mut line = if stack.thread_name.is_empty() {
                format!("{:?}", stack.thread_id)
            } else {
                stack.thread_name.clone()
            };
            for frame in stack.frames.iter().rev() {
                for symbol in frame.iter().rev() {
                    line.push(';');
                    line.push_str(&symbol.to_string());
                }
            }
            line.push(' ');
            line.push_str(&count.to_string());
            line
        })
        .collect();
    lines.sort_unstable();
    lines
}

/// Criterion's profiler under `--profile-time`: each bench sampled, and its flamegraph written
/// into the bench's directory. A sampler that can't start, or a graph that can't be written, is
/// reported on stderr and leaves the bench running.
#[derive(Default)]
pub struct Flamegraph {
    guard: Option<ProfilerGuard<'static>>,
}

impl Profiler for Flamegraph {
    fn start_profiling(&mut self, benchmark_id: &str, _benchmark_dir: &Path) {
        match sample() {
            Ok(guard) => self.guard = Some(guard),
            Err(err) => eprintln!("{benchmark_id}: cannot start the profiler: {err}"),
        }
    }

    fn stop_profiling(&mut self, benchmark_id: &str, benchmark_dir: &Path) {
        let Some(guard) = self.guard.take() else {
            return;
        };
        let path = benchmark_dir.join("flamegraph.svg");
        match write_flamegraph(&guard, &path) {
            Ok(()) => println!("{benchmark_id}: flamegraph in {}", path.display()),
            Err(err) => eprintln!("{benchmark_id}: cannot write {}: {err}", path.display()),
        }
    }
}
