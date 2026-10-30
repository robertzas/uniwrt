//! Command-line interface. Every option can also come from a `UNIWRT_*` environment variable.

use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use ipnet::IpNet;
use uniwrt_sim::SimConfig;

use crate::{build_info, tls::names::TlsName};

const DEFAULT_HTTP_LISTEN: &str = "0.0.0.0:8080";
const DEFAULT_HTTPS_LISTEN: &str = "0.0.0.0:8443";

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
    /// Address and port of the HTTP listener, which redirects to HTTPS except for the health
    /// check and trusted proxies.
    #[arg(long, env = "UNIWRT_HTTP_LISTEN", value_name = "ADDR:PORT", default_value = DEFAULT_HTTP_LISTEN)]
    pub(crate) http_listen: SocketAddr,
    /// Address and port of the HTTPS listener, which serves the UI and the API.
    #[arg(long, env = "UNIWRT_HTTPS_LISTEN", value_name = "ADDR:PORT", default_value = DEFAULT_HTTPS_LISTEN)]
    pub(crate) https_listen: SocketAddr,
    /// More DNS names and IP addresses for the HTTPS certificate, comma-separated: the ones
    /// clients use that the controller cannot see itself (the host's address when it runs in
    /// a bridged container, a DNS name). Configure them before the first run: the local CA may
    /// sign only local names, private addresses and the names configured when it was made.
    #[arg(
        long,
        env = "UNIWRT_TLS_NAMES",
        value_name = "NAME",
        value_delimiter = ','
    )]
    pub(crate) tls_names: Vec<TlsName>,
    /// Reverse proxies that terminate TLS and forward to the HTTP listener, comma-separated
    /// networks (`127.0.0.1/32`, `172.16.0.0/12`); their requests get the whole application.
    #[arg(
        long,
        env = "UNIWRT_TRUSTED_PROXIES",
        value_name = "CIDR",
        value_delimiter = ','
    )]
    pub(crate) trusted_proxies: Vec<IpNet>,
    /// The vault's master key file (default: `master.key` in the data directory), created on
    /// the first run. `UNIWRT_MASTER_KEY` may hold the key itself (64 hexadecimal digits).
    #[arg(long, env = "UNIWRT_MASTER_KEY_FILE", value_name = "FILE")]
    pub(crate) master_key_file: Option<PathBuf>,
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
    /// Run a simulated fleet (a gateway, 3 access points and 30 clients) instead of the adopted
    /// nodes. Its database lives in a temporary directory inside the data directory and is
    /// deleted at exit; the real data is never touched.
    #[arg(long, env = "UNIWRT_DEMO")]
    pub(crate) demo: bool,
    /// The simulation's seed: the same seed gives the same fleet and story.
    #[arg(long, env = "UNIWRT_DEMO_SEED", value_name = "SEED", default_value_t = SimConfig::DEFAULT_SEED, requires = "demo")]
    pub(crate) demo_seed: u64,
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
    use crate::tls::names::TlsName;

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
        assert_eq!(args.https_listen.to_string(), "0.0.0.0:8443");
        assert!(args.tls_names.is_empty() && args.trusted_proxies.is_empty());
        assert!(args.master_key_file.is_none());
    }

    #[test]
    fn https_takes_extra_names_and_trusted_proxies_as_lists() {
        let cli = Cli::try_parse_from([
            "uniwrt",
            "--https-listen",
            "[::]:443",
            "--tls-names",
            "192.168.1.10,uniwrt.home.arpa",
            "--trusted-proxies",
            "127.0.0.1/32,fd00::/8",
            "--master-key-file",
            "/run/secrets/master.key",
        ])
        .unwrap();
        let args = cli.serve;
        assert_eq!(args.https_listen.to_string(), "[::]:443");
        assert_eq!(
            args.tls_names,
            [
                TlsName::Ip("192.168.1.10".parse().unwrap()),
                TlsName::Dns("uniwrt.home.arpa".into())
            ]
        );
        assert_eq!(args.trusted_proxies.len(), 2);
        assert_eq!(
            args.master_key_file.as_deref().and_then(|p| p.to_str()),
            Some("/run/secrets/master.key")
        );
        assert!(Cli::try_parse_from(["uniwrt", "--tls-names", "*.lan"]).is_err());
        assert!(Cli::try_parse_from(["uniwrt", "--trusted-proxies", "10.0.0.1"]).is_err());
    }

    #[test]
    fn demo_mode_takes_a_seed() {
        let cli = Cli::try_parse_from(["uniwrt", "--demo"]).unwrap();
        assert!(cli.serve.demo);
        assert_eq!(cli.serve.demo_seed, uniwrt_sim::SimConfig::DEFAULT_SEED);
        let cli = Cli::try_parse_from(["uniwrt", "serve", "--demo", "--demo-seed", "7"]).unwrap();
        assert!(
            matches!(cli.command, Some(Command::Serve(args)) if args.demo && args.demo_seed == 7)
        );
        assert!(Cli::try_parse_from(["uniwrt", "--demo-seed", "7"]).is_err());
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
