//! The `uniwrt` binary: CLI, configuration and composition root; serves the API and the web UI.
//!
//! - **Commands:** `serve` (the default; in release builds under the `launcher`, which runs the
//!   newest verified binary of the controller), `healthcheck` (exit code 0/1 for Docker) and
//!   `version`.
//!   Every flag has a `UNIWRT_*` environment equivalent (`cli`). Only `serve` starts a runtime.
//! - **Modules:** `serve` (runtime, database, listeners, graceful shutdown) → `http` (router: `/api`,
//!   security headers; `http::plain` redirects plain HTTP) → `ui` (embedded assets, content
//!   negotiation, SPA fallback). `private` keeps every file it writes its user's (umask 077); `master_key` opens the vault, `controller_key` keeps the SSH
//!   key in it, and `tls` keeps the local CA and the HTTPS certificate there and serves them
//!   (D-039). `build_info` and the build script in `build/` provide the version, commit and the
//!   embedded UI.
//! - **Invariants:** no `unsafe`; files are written only under the data dir; release builds embed
//!   the real UI because CI sets `UNIWRT_WEB_DIST`, which fails the build without `index.html`.
//! - **Tests:** unit tests sit next to each module. Router tests drive `http::router` with
//!   `tower::ServiceExt::oneshot`; serve and healthcheck tests use loopback sockets.

mod adoption_port;
mod auto_update;
mod autotune;
mod blocklists;
mod build_info;
mod cli;
mod controller_backups;
mod controller_key;
mod demo;
#[cfg(test)]
mod demo_backhaul_tests;
#[cfg(test)]
mod demo_port_tests;
mod demo_site;
mod feeds;
mod feeds_next;
mod fleet;
mod fleet_link;
mod healthcheck;
mod http;
mod launcher;
mod logging;
mod master_key;
mod nightly;
mod private;
#[cfg(feature = "profiling")]
mod profiling;
mod recollect;
mod restore;
mod scheduled_backups;
mod scheduled_reboots;
mod serve;
mod tls;
mod ui;
mod version;

// The build script's date formatting, unit-tested as part of this crate.
#[cfg(test)]
#[path = "../build/civil.rs"]
mod build_civil;

use std::process::ExitCode;

use clap::Parser as _;

use crate::cli::{Cli, Command};

/// mimalloc (D-128): about a fifth less CPU per request than musl's malloc, for about twice the
/// memory; jemalloc saved as much CPU but took more memory still.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    // No transparent huge pages (D-149, M9.2d): mimalloc madvises its arenas for them, which
    // doubled the heap's RSS (69 → 31 MB idle in CI, 217–242 → 153–175 MB under the API's reads) for no
    // measurable CPU. Process-wide, from here on, and kept across the launcher's exec; an old
    // kernel without the prctl keeps the system's setting.
    let _ = rustix::thread::disable_transparent_huge_pages(true);
    private::restrict_new_files();
    let cli = Cli::parse();
    let result = match cli.command {
        None => launcher::run(cli.serve),
        Some(Command::Serve(args)) => launcher::run(*args),
        Some(Command::Healthcheck(args)) => healthcheck::run(&args).map(|()| 0),
        Some(Command::Version(args)) => version::run(&args).map(|()| 0),
        Some(Command::Restore(args)) => restore::run(&args).map(|()| 0),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("uniwrt: {err:#}");
            ExitCode::FAILURE
        }
    }
}
