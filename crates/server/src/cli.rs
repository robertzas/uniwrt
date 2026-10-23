//! Command-line interface. Every option can also come from a `UNIWRT_*` environment variable.

use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::build_info;

const DEFAULT_HTTP_LISTEN: &str = "0.0.0.0:8080";

#[derive(Debug, Parser)]
#[command(
    name = "uniwrt",
    version = build_info::VERSION,
    about = "UniFi-class controller for OpenWrt mesh networks",
    args_conflicts_with_subcommands = true
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
    /// Options for `serve`, which runs when no command is given.
    #[command(flatten)]
    pub(crate) serve: ServeArgs,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Run the controller (the default).
    Serve(ServeArgs),
    /// Exit 0 if the local server answers `GET /api/v1/health` with 200, else 1 (for Docker).
    Healthcheck(HealthcheckArgs),
    /// Print the version and build information.
    Version(VersionArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ServeArgs {
    /// Address and port of the HTTP listener.
    #[arg(long, env = "UNIWRT_HTTP_LISTEN", value_name = "ADDR:PORT", default_value = DEFAULT_HTTP_LISTEN)]
    pub(crate) http_listen: SocketAddr,
    /// Directory for the database, keys, backups and the firmware cache.
    #[arg(
        long,
        env = "UNIWRT_DATA_DIR",
        value_name = "DIR",
        default_value = "./data"
    )]
    pub(crate) data_dir: PathBuf,
    /// Log filter in tracing's `EnvFilter` syntax, for example `info,uniwrt=debug`.
    #[arg(
        long,
        env = "UNIWRT_LOG",
        value_name = "FILTER",
        default_value = "info"
    )]
    pub(crate) log: String,
}

#[derive(Debug, Args)]
pub(crate) struct HealthcheckArgs {
    /// The server's HTTP listen address; an unspecified IP (0.0.0.0 or ::) is probed on loopback.
    #[arg(long, env = "UNIWRT_HTTP_LISTEN", value_name = "ADDR:PORT", default_value = DEFAULT_HTTP_LISTEN)]
    pub(crate) http_listen: SocketAddr,
    /// Seconds to wait for the connection and for the response.
    #[arg(long, value_name = "SECS", default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=60))]
    pub(crate) timeout: u64,
}

#[derive(Debug, Args)]
pub(crate) struct VersionArgs {
    /// Print JSON instead of text.
    #[arg(long)]
    pub(crate) json: bool,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory as _, Parser as _};

    use super::{Cli, Command};

    #[test]
    fn definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn serve_flags_work_with_and_without_the_subcommand() {
        let cli = Cli::try_parse_from(["uniwrt", "--http-listen", "[::1]:8081"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.serve.http_listen.to_string(), "[::1]:8081");

        let cli = Cli::try_parse_from([
            "uniwrt",
            "serve",
            "--http-listen",
            "127.0.0.1:9000",
            "--data-dir",
            "/srv/uniwrt",
            "--log",
            "debug",
        ])
        .unwrap();
        let Some(Command::Serve(args)) = cli.command else {
            panic!("expected serve")
        };
        assert_eq!(args.http_listen.to_string(), "127.0.0.1:9000");
        assert_eq!(args.data_dir.to_str(), Some("/srv/uniwrt"));
        assert_eq!(args.log, "debug");
    }

    #[test]
    fn other_commands_parse() {
        let cli = Cli::try_parse_from(["uniwrt", "healthcheck", "--timeout", "5"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Healthcheck(args)) if args.timeout == 5));
        let cli = Cli::try_parse_from(["uniwrt", "version", "--json"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Version(args)) if args.json));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Cli::try_parse_from(["uniwrt", "--http-listen", "localhost"]).is_err());
        assert!(Cli::try_parse_from(["uniwrt", "healthcheck", "--timeout", "0"]).is_err());
        assert!(Cli::try_parse_from(["uniwrt", "--log", "x", "version"]).is_err());
    }
}
