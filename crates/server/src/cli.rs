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
    Serve(Box<ServeArgs>),
    /// Exit 0 if the local server answers `GET /api/v1/health` with 200, else 1 (for Docker).
    Healthcheck(HealthcheckArgs),
    /// Print the version and build information.
    Version(VersionArgs),
    /// Stage a controller backup (`uniwrt-backup-….tar.gz.age`) for the next start, which puts
    /// it in place. The passphrase comes from `UNIWRT_BACKUP_PASSPHRASE` or stdin.
    Restore(RestoreArgs),
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
    /// Address and port of the syslog receiver, over UDP and TCP, or `off`. Default:
    /// `0.0.0.0:5514`, and off in demo mode.
    #[arg(long, env = "UNIWRT_SYSLOG_LISTEN", value_name = "ADDR:PORT|off", value_parser = parse_syslog_listen)]
    pub(crate) syslog_listen: Option<SyslogListen>,
    /// Address and port of the controller's iperf3 server, or `off`: it listens there only while
    /// a node ↔ controller throughput test runs, for that node alone (D-142); nodes must reach
    /// it (open the port in the host's firewall, publish it from a container). Port 0 takes any
    /// free port for each test.
    #[arg(
        long,
        env = "UNIWRT_IPERF3_LISTEN",
        value_name = "ADDR:PORT|off",
        default_value = "[::]:5201",
        value_parser = parse_iperf3_listen
    )]
    pub(crate) iperf3_listen: Iperf3Listen,
    /// The Attended Sysupgrade (ASU) service that builds zero-touch images, or `off`. Nothing
    /// secret is sent to it.
    #[arg(
        long,
        env = "UNIWRT_ASU_URL",
        value_name = "URL|off",
        default_value = uniwrt_firmware::Asu::PUBLIC
    )]
    pub(crate) asu_url: String,
    /// The vault's master key file (default: `master.key` in the data directory), created on
    /// the first run. `UNIWRT_MASTER_KEY` may hold the key itself (64 hexadecimal digits).
    #[arg(long, env = "UNIWRT_MASTER_KEY_FILE", value_name = "FILE")]
    pub(crate) master_key_file: Option<PathBuf>,
    /// Whether the controller may upgrade itself from its GitHub releases (D-121, D-123): `on`
    /// runs `serve` under a small launcher that starts the newest verified binary staged in
    /// `<data dir>/bin` and falls back to this one when it fails its health check; `off` serves
    /// directly (a package manager owns the binary). Off in demo mode and in development builds.
    #[arg(
        long,
        env = "UNIWRT_SELF_UPGRADE",
        value_name = "on|off",
        default_value = "on",
        value_parser = parse_on_off
    )]
    pub(crate) self_upgrade: bool,
    /// Directory for the database, keys, backups and the firmware cache.
    #[arg(
        long,
        env = "UNIWRT_DATA_DIR",
        value_name = "DIR",
        default_value = "./data"
    )]
    pub(crate) data_dir: PathBuf,
    /// Another directory that each controller backup is copied to (a NAS mount, say); the last
    /// 7 are kept there too. Backups are always written to `<data dir>/backups` first.
    #[arg(long, env = "UNIWRT_BACKUP_DIR", value_name = "DIR")]
    pub(crate) backup_dir: Option<PathBuf>,
    /// Log filter in tracing's `EnvFilter` syntax, for example `info,uniwrt=debug`.
    #[arg(
        long,
        env = "UNIWRT_LOG",
        value_name = "FILTER",
        default_value = "info"
    )]
    pub(crate) log: String,
    /// Sample the CPU from start to exit and write a flamegraph (SVG) to FILE: builds with the
    /// `profiling` feature only (`just profile serve`, D-149).
    #[cfg(feature = "profiling")]
    #[arg(long, value_name = "FILE")]
    pub(crate) profile_cpu: Option<PathBuf>,
    /// Run a simulated fleet (a gateway, 3 access points and 30 clients) instead of the adopted
    /// nodes. Its database lives in a temporary directory inside the data directory and is
    /// deleted at exit; the real data is never touched.
    #[arg(long, env = "UNIWRT_DEMO")]
    pub(crate) demo: bool,
    /// The simulation's seed: the same seed gives the same fleet and story.
    #[arg(long, env = "UNIWRT_DEMO_SEED", value_name = "SEED", default_value_t = SimConfig::DEFAULT_SEED, requires = "demo")]
    pub(crate) demo_seed: u64,
    /// How many access points the simulated fleet has (3 by default); the last one waits to be
    /// adopted. Sized fleets check the performance budgets (D-149).
    #[arg(long, env = "UNIWRT_DEMO_ACCESS_POINTS", value_name = "N", requires = "demo", conflicts_with = "demo_factory", value_parser = clap::value_parser!(u16).range(1..=250))]
    pub(crate) demo_access_points: Option<u16>,
    /// How many clients the simulated fleet has (30 by default, 12 with `--demo-factory`).
    #[arg(long, env = "UNIWRT_DEMO_CLIENTS", value_name = "N", requires = "demo", value_parser = clap::value_parser!(u16).range(0..=10_000))]
    pub(crate) demo_clients: Option<u16>,
    /// Simulate a factory-default gateway alone (192.168.1.1, root without a password) with
    /// nothing adopted and no user, so that the first run happens as on a new network: create
    /// the admin, then set the router up in the wizard.
    #[arg(long, env = "UNIWRT_DEMO_FACTORY", requires = "demo")]
    pub(crate) demo_factory: bool,
}

/// Where the syslog receiver listens, if at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyslogListen {
    Off,
    On(SocketAddr),
}

impl ServeArgs {
    /// The syslog receiver's address: as configured, else the default (none in demo mode).
    pub(crate) fn syslog_addr(&self) -> Option<SocketAddr> {
        match self.syslog_listen {
            Some(SyslogListen::On(addr)) => Some(addr),
            Some(SyslogListen::Off) => None,
            None => (!self.demo).then(|| SocketAddr::from(([0, 0, 0, 0], 5514))),
        }
    }
}

fn parse_on_off(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        _ => Err("expected on or off".to_owned()),
    }
}

/// Where the controller's iperf3 server listens during a test, if at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Iperf3Listen(pub(crate) Option<SocketAddr>);

fn parse_iperf3_listen(value: &str) -> Result<Iperf3Listen, String> {
    if value.eq_ignore_ascii_case("off") {
        return Ok(Iperf3Listen(None));
    }
    value
        .parse()
        .map(|addr| Iperf3Listen(Some(addr)))
        .map_err(|_| format!("{value:?} is not ADDR:PORT or off"))
}

fn parse_syslog_listen(value: &str) -> Result<SyslogListen, String> {
    if value.eq_ignore_ascii_case("off") {
        return Ok(SyslogListen::Off);
    }
    value
        .parse()
        .map(SyslogListen::On)
        .map_err(|_| format!("{value:?} is not ADDR:PORT or off"))
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
pub(crate) struct RestoreArgs {
    /// The backup file.
    #[arg(value_name = "FILE")]
    pub(crate) file: PathBuf,
    /// The data directory of the controller to restore into.
    #[arg(
        long,
        env = "UNIWRT_DATA_DIR",
        value_name = "DIR",
        default_value = "./data"
    )]
    pub(crate) data_dir: PathBuf,
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
        assert_eq!(
            args.syslog_addr().map(|a| a.to_string()).as_deref(),
            Some("0.0.0.0:5514")
        );
    }

    #[test]
    fn syslog_listens_where_told_or_not_at_all() {
        let serve = |args: &[&str]| {
            let cli = Cli::try_parse_from([&["uniwrt", "serve"], args].concat()).unwrap();
            let Some(Command::Serve(args)) = cli.command else {
                panic!("expected serve")
            };
            args.syslog_addr()
        };
        assert_eq!(
            serve(&["--syslog-listen", "127.0.0.1:1514"]),
            "127.0.0.1:1514".parse().ok()
        );
        assert_eq!(serve(&["--syslog-listen", "off"]), None);
        // Off in demo mode unless asked for.
        assert_eq!(serve(&["--demo"]), None);
        assert!(serve(&["--demo", "--syslog-listen", "[::1]:5514"]).is_some());
        assert!(Cli::try_parse_from(["uniwrt", "serve", "--syslog-listen", "5514"]).is_err());
    }

    #[test]
    fn the_iperf3_server_listens_where_told_or_not_at_all() {
        let iperf3 = |args: &[&str]| {
            let Cli {
                command: Some(Command::Serve(args)),
                ..
            } = Cli::try_parse_from([&["uniwrt", "serve"], args].concat()).unwrap()
            else {
                panic!("not serve");
            };
            args.iperf3_listen.0
        };
        assert_eq!(iperf3(&[]), Some("[::]:5201".parse().unwrap()));
        assert_eq!(iperf3(&["--iperf3-listen", "off"]), None);
        assert_eq!(
            iperf3(&["--iperf3-listen", "192.168.1.2:0"]),
            Some("192.168.1.2:0".parse().unwrap())
        );
        assert!(Cli::try_parse_from(["uniwrt", "serve", "--iperf3-listen", "5201"]).is_err());
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
        let cli = Cli::try_parse_from(["uniwrt", "--demo", "--demo-factory"]).unwrap();
        assert!(cli.serve.demo_factory);
        assert!(Cli::try_parse_from(["uniwrt", "--demo-factory"]).is_err());
        let cli = Cli::try_parse_from([
            "uniwrt",
            "--demo",
            "--demo-access-points",
            "10",
            "--demo-clients",
            "200",
        ])
        .unwrap();
        assert_eq!(
            (cli.serve.demo_access_points, cli.serve.demo_clients),
            (Some(10), Some(200))
        );
        assert!(Cli::try_parse_from(["uniwrt", "--demo-clients", "5"]).is_err());
        assert!(Cli::try_parse_from(["uniwrt", "--demo", "--demo-access-points", "0"]).is_err());
        let factory = ["--demo", "--demo-factory", "--demo-access-points", "2"];
        assert!(Cli::try_parse_from(std::iter::once("uniwrt").chain(factory)).is_err());
    }

    #[test]
    fn other_commands_parse() {
        let cli = Cli::try_parse_from(["uniwrt", "healthcheck", "--timeout", "5"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Healthcheck(args)) if args.timeout == 5));
        let cli = Cli::try_parse_from(["uniwrt", "version", "--json"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Version(args)) if args.json));
        let cli = Cli::try_parse_from(["uniwrt", "restore", "b.tar.gz.age", "--data-dir", "/srv"])
            .unwrap();
        assert!(matches!(cli.command, Some(Command::Restore(args))
            if args.file.to_str() == Some("b.tar.gz.age") && args.data_dir.to_str() == Some("/srv")));
        let cli = Cli::try_parse_from(["uniwrt", "--backup-dir", "/mnt/nas"]).unwrap();
        assert_eq!(
            cli.serve.backup_dir.as_deref().and_then(|p| p.to_str()),
            Some("/mnt/nas")
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Cli::try_parse_from(["uniwrt", "--http-listen", "localhost"]).is_err());
        assert!(Cli::try_parse_from(["uniwrt", "healthcheck", "--timeout", "0"]).is_err());
        assert!(Cli::try_parse_from(["uniwrt", "--log", "x", "version"]).is_err());
    }
}
