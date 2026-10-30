//! The `uniwrt` binary: CLI, configuration and composition root; serves the API and the web UI.
//!
//! - **Commands:** `serve` (the default), `healthcheck` (exit code 0/1 for Docker) and `version`.
//!   Every flag has a `UNIWRT_*` environment equivalent (`cli`). Only `serve` starts a runtime.
//! - **Modules:** `serve` (runtime, database, listeners, graceful shutdown) → `http` (router: `/api`,
//!   security headers; `http::plain` redirects plain HTTP) → `ui` (embedded assets, content
//!   negotiation, SPA fallback). `master_key` opens the vault, `controller_key` keeps the SSH
//!   key in it, and `tls` keeps the local CA and the HTTPS certificate there and serves them
//!   (D-039). `build_info` and the build script in `build/` provide the version, commit and the
//!   embedded UI.
//! - **Invariants:** no `unsafe`; files are written only under the data dir; release builds embed
//!   the real UI because CI sets `UNIWRT_WEB_DIST`, which fails the build without `index.html`.
//! - **Tests:** unit tests sit next to each module. Router tests drive `http::router` with
//!   `tower::ServiceExt::oneshot`; serve and healthcheck tests use loopback sockets.

mod build_info;
mod cli;
mod controller_key;
mod demo;
mod fleet;
mod fleet_link;
mod healthcheck;
mod http;
mod logging;
mod master_key;
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        None => serve::run(cli.serve),
        Some(Command::Serve(args)) => serve::run(args),
        Some(Command::Healthcheck(args)) => healthcheck::run(&args),
        Some(Command::Version(args)) => version::run(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("uniwrt: {err:#}");
            ExitCode::FAILURE
        }
    }
}
