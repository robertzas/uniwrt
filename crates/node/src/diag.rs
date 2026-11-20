//! The commands of the diagnostics toolkit's tools (features/observability.md, Diagnostics
//! toolkit; D-141), as argv arrays for [`NodeLink::stream`](crate::NodeLink::stream): BusyBox's
//! `ping`, `traceroute` and `nslookup` and `uclient-fetch`, which OpenWrt's images carry
//! (checked on 24.10 and 25.12). The tool is validated first ([`DiagTool::validate`]): no
//! argument can be taken for an option.
//!
//! Closing an SSH channel doesn't end its command on dropbear, so each tool runs under
//! [`WATCHDOG`], a fixed script that ends it when the channel's stdin closes (the controller
//! stopped the run, or lost the node). The tool's argv reaches it as `"$@"`, never as script
//! text.
//!
//! [`tables`] reads the neighbor, route and connection tables in one exec each (D-142);
//! [`capture`] runs tcpdump under the same watchdog, its filter extended to leave out the
//! controller's own SSH session.

/// The watchdog's script with `$setup` run first, which may rearrange `"$@"`.
macro_rules! guarded {
    ($setup:literal) => {
        concat!(
            "exec 3<&0 0</dev/null\n",
            $setup,
            "\"$@\" & tool=$!
{ while read -r _; do :; done <&3; kill \"$tool\" 2>/dev/null; } &
watch=$!
wait \"$tool\"; status=$?
kill \"$watch\" 2>/dev/null
exit \"$status\""
        )
    };
}

pub mod capture;
pub mod tables;

use std::time::Duration;

use uniwrt_model::diag::{DiagTool, IpFamily};

/// How long a traceroute waits for each probe, in seconds.
const TRACE_WAIT_S: u32 = 2;
/// Probes per hop.
const TRACE_PROBES: u32 = 2;
/// The most hops a traceroute follows.
const TRACE_MAX_HOPS: u32 = 30;
/// How long the HTTP check waits to connect and for each read, in seconds.
const HTTP_TIMEOUT_S: u32 = 10;
/// How long a cable test may take: a PHY reports within seconds, slow ones within half a minute.
const CABLE_TEST_S: u64 = 60;

/// Runs `"$@"` with stdin from /dev/null, and ends it (SIGTERM) once the shell's own stdin
/// reaches its end; exits as the tool did.
pub const WATCHDOG: &str = guarded!("");

/// The command that runs `tool` on a node, under [`WATCHDOG`].
pub fn command(tool: &DiagTool) -> Vec<String> {
    let mut argv: Vec<String> = ["sh", "-c", WATCHDOG, "sh"].map(str::to_owned).into();
    argv.extend(tool_command(tool));
    argv
}

/// The tool's own argv in [`command`]'s (or [`capture::command`]'s, without its filter).
pub fn tool_argv(argv: &[String]) -> &[String] {
    match argv {
        [sh, c, script, _, tool @ ..] if sh == "sh" && c == "-c" && script == WATCHDOG => tool,
        [sh, c, script, _, _filter, tool @ ..]
            if sh == "sh" && c == "-c" && script == capture::CAPTURE =>
        {
            tool
        }
        other => other,
    }
}

/// The tool's own argv.
fn tool_command(tool: &DiagTool) -> Vec<String> {
    let family = |family: &Option<IpFamily>| match family {
        Some(IpFamily::Ipv4) => Some("-4".to_owned()),
        Some(IpFamily::Ipv6) => Some("-6".to_owned()),
        None => None,
    };
    let mut argv: Vec<String> = Vec::new();
    match tool {
        DiagTool::Ping {
            target,
            family: f,
            count,
        } => {
            // `-w` ends it even when replies never come.
            argv.extend(["ping", "-c"].map(str::to_owned));
            argv.push(count.to_string());
            argv.push("-w".to_owned());
            argv.push((count + 5).to_string());
            argv.extend(family(f));
            argv.push(target.clone());
        }
        DiagTool::Traceroute { target, family: f } => {
            argv.extend(["traceroute", "-n", "-w"].map(str::to_owned));
            argv.push(TRACE_WAIT_S.to_string());
            argv.push("-q".to_owned());
            argv.push(TRACE_PROBES.to_string());
            argv.push("-m".to_owned());
            argv.push(TRACE_MAX_HOPS.to_string());
            argv.extend(family(f));
            argv.push(target.clone());
        }
        DiagTool::Dns {
            name,
            record,
            server,
        } => {
            argv.push("nslookup".to_owned());
            argv.extend(record.map(|record| format!("-type={}", record.as_str())));
            argv.push(name.clone());
            argv.extend(server.clone());
        }
        DiagTool::Http { url } => {
            // 24.10's `--spider` downloads the body anyway, so it goes to /dev/null.
            argv.extend(["uclient-fetch", "-O", "/dev/null", "-T"].map(str::to_owned));
            argv.push(HTTP_TIMEOUT_S.to_string());
            argv.push(url.clone());
        }
        DiagTool::CableTest { port } => {
            argv.extend(["ethtool", "--cable-test"].map(str::to_owned));
            argv.push(port.clone());
        }
    }
    argv
}

/// How long `tool` may run before the controller stops it: its own bounds, with a margin.
pub fn limit(tool: &DiagTool) -> Duration {
    let secs = match tool {
        DiagTool::Ping { count, .. } => u64::from(*count) + 15,
        DiagTool::Traceroute { .. } => u64::from(TRACE_WAIT_S * TRACE_PROBES * TRACE_MAX_HOPS) + 30,
        DiagTool::Dns { .. } => 30,
        DiagTool::Http { .. } => 60,
        DiagTool::CableTest { .. } => CABLE_TEST_S,
    };
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uniwrt_model::diag::{DiagTool, DnsRecordType, IpFamily};

    use super::{WATCHDOG, command, limit, tool_argv, tool_command as command_of};

    #[test]
    fn tools_run_under_the_watchdog() {
        let ping = DiagTool::Ping {
            target: "1.1.1.1".to_owned(),
            family: None,
            count: 1,
        };
        let argv = command(&ping);
        assert_eq!(argv[..4], ["sh", "-c", WATCHDOG, "sh"]);
        assert_eq!(
            WATCHDOG,
            "exec 3<&0 0</dev/null\n\"$@\" & tool=$!\n{ while read -r _; do :; done <&3; kill \"$tool\" 2>/dev/null; } &\nwatch=$!\nwait \"$tool\"; status=$?\nkill \"$watch\" 2>/dev/null\nexit \"$status\""
        );
        assert_eq!(tool_argv(&argv), command_of(&ping));
        assert_eq!(tool_argv(&argv[4..]), command_of(&ping));
    }

    #[test]
    fn each_tool_runs_as_checked_on_openwrt() {
        let ping = DiagTool::Ping {
            target: "1.1.1.1".to_owned(),
            family: Some(IpFamily::Ipv4),
            count: 4,
        };
        assert_eq!(
            command_of(&ping),
            ["ping", "-c", "4", "-w", "9", "-4", "1.1.1.1"]
        );
        assert_eq!(limit(&ping), Duration::from_secs(19));
        let trace = DiagTool::Traceroute {
            target: "openwrt.org".to_owned(),
            family: Some(IpFamily::Ipv6),
        };
        assert_eq!(
            command_of(&trace),
            [
                "traceroute",
                "-n",
                "-w",
                "2",
                "-q",
                "2",
                "-m",
                "30",
                "-6",
                "openwrt.org"
            ]
        );
        assert_eq!(limit(&trace), Duration::from_secs(150));
        let dns = DiagTool::Dns {
            name: "openwrt.org".to_owned(),
            record: Some(DnsRecordType::Aaaa),
            server: Some("9.9.9.9".to_owned()),
        };
        assert_eq!(
            command_of(&dns),
            ["nslookup", "-type=AAAA", "openwrt.org", "9.9.9.9"]
        );
        let dns = DiagTool::Dns {
            name: "router.lan".to_owned(),
            record: None,
            server: None,
        };
        assert_eq!(command_of(&dns), ["nslookup", "router.lan"]);
        let http = DiagTool::Http {
            url: "https://openwrt.org/".to_owned(),
        };
        assert_eq!(
            command_of(&http),
            [
                "uclient-fetch",
                "-O",
                "/dev/null",
                "-T",
                "10",
                "https://openwrt.org/"
            ]
        );
        let cable = DiagTool::CableTest {
            port: "lan1".to_owned(),
        };
        assert_eq!(command_of(&cable), ["ethtool", "--cable-test", "lan1"]);
        assert_eq!(limit(&cable), Duration::from_mins(1));
    }
}
