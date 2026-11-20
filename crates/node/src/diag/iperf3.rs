//! iperf3's commands (D-142): the client on the run's node, and for a test between nodes the
//! server on the other, `iperf3 -s -1` (one test, then it exits) on [`PORT`], which prints
//! [`LISTENING`] once it listens (`--forceflush`: at once, even into a pipe; checked on 24.10
//! and 25.12). The client prints its intervals as they pass, in Mbit/s, and both ends' totals.
//!
//! Against the controller, the client runs under [`TO_CONTROLLER`], the watchdog's script with
//! a setup of its own: the controller's address as the node sees it, from dropbear's
//! `$SSH_CONNECTION` (or `$SSH_CLIENT`), reaches iperf3 as `-c <address>`. The controller's own
//! server ([`crate::iperf3`]) listens there for the test.

use uniwrt_model::diag::iperf3::Iperf3Direction;

/// The port the far node's server listens on: iperf3's own.
pub const PORT: u16 = 5201;
/// How long the client waits to connect, in milliseconds.
const CONNECT_TIMEOUT_MS: u32 = 5000;
/// What the server prints once it listens.
pub const LISTENING: &str = "Server listening on";

/// Runs `"$@" -c <controller>` under the watchdog, the controller's address read from the SSH
/// session (a scope and a `::ffff:` prefix dropped).
pub const TO_CONTROLLER: &str = guarded!(
    "c=${SSH_CONNECTION:-${SSH_CLIENT:-}}
ip=${c%% *}; ip=${ip%%[%]*}; ip=${ip#::ffff:}
[ -n \"$ip\" ] || { echo 'uniwrt: the SSH session names no controller address' >&2; exit 2; }
set -- \"$@\" -c \"$ip\"
"
);

/// The far end of a test, as the client reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Far<'a> {
    /// Another node, at the address the controller manages it at, its server on [`PORT`].
    Node { host: &'a str },
    /// The controller's server, listening on `port`.
    Controller { port: u16 },
}

/// The client's options, without the far end's address.
pub(super) fn options(
    direction: Iperf3Direction,
    seconds: u32,
    streams: u32,
    port: u16,
) -> Vec<String> {
    let mut argv: Vec<String> = vec!["iperf3".to_owned(), "-p".to_owned(), port.to_string()];
    argv.push("-t".to_owned());
    argv.push(seconds.to_string());
    argv.push("-P".to_owned());
    argv.push(streams.to_string());
    match direction {
        Iperf3Direction::Send => {}
        Iperf3Direction::Receive => argv.push("-R".to_owned()),
        Iperf3Direction::Both => argv.push("--bidir".to_owned()),
    }
    argv.extend(["-f", "m", "-i", "1", "--forceflush", "--connect-timeout"].map(str::to_owned));
    argv.push(CONNECT_TIMEOUT_MS.to_string());
    argv
}

/// The client's command on the run's node.
pub fn client(direction: Iperf3Direction, seconds: u32, streams: u32, far: Far<'_>) -> Vec<String> {
    match far {
        Far::Node { host } => {
            let mut argv: Vec<String> = ["sh", "-c", super::WATCHDOG, "sh"]
                .map(str::to_owned)
                .into();
            argv.extend(options(direction, seconds, streams, PORT));
            argv.push("-c".to_owned());
            argv.push(host.to_owned());
            argv
        }
        Far::Controller { port } => {
            let mut argv: Vec<String> = ["sh", "-c", TO_CONTROLLER, "sh"].map(str::to_owned).into();
            argv.extend(options(direction, seconds, streams, port));
            argv
        }
    }
}

/// The far node's server: one test on [`PORT`], then it exits.
pub fn server() -> Vec<String> {
    let mut argv: Vec<String> = ["sh", "-c", super::WATCHDOG, "sh"]
        .map(str::to_owned)
        .into();
    argv.extend(["iperf3", "-s", "-1", "-p"].map(str::to_owned));
    argv.push(PORT.to_string());
    argv.extend(["-f", "m", "--forceflush"].map(str::to_owned));
    argv
}

#[cfg(test)]
mod tests {
    use uniwrt_model::diag::iperf3::Iperf3Direction;

    use super::{Far, TO_CONTROLLER, client, server};
    use crate::diag::{WATCHDOG, tool_argv};

    #[test]
    fn the_client_reaches_a_node_or_the_controller() {
        let argv = client(
            Iperf3Direction::Receive,
            10,
            2,
            Far::Node {
                host: "192.168.1.2",
            },
        );
        assert_eq!(argv[..4], ["sh", "-c", WATCHDOG, "sh"]);
        assert_eq!(
            tool_argv(&argv),
            [
                "iperf3",
                "-p",
                "5201",
                "-t",
                "10",
                "-P",
                "2",
                "-R",
                "-f",
                "m",
                "-i",
                "1",
                "--forceflush",
                "--connect-timeout",
                "5000",
                "-c",
                "192.168.1.2"
            ]
        );
        let argv = client(
            Iperf3Direction::Both,
            5,
            1,
            Far::Controller { port: 45_000 },
        );
        assert_eq!(argv[..4], ["sh", "-c", TO_CONTROLLER, "sh"]);
        assert_eq!(
            tool_argv(&argv),
            [
                "iperf3",
                "-p",
                "45000",
                "-t",
                "5",
                "-P",
                "1",
                "--bidir",
                "-f",
                "m",
                "-i",
                "1",
                "--forceflush",
                "--connect-timeout",
                "5000"
            ]
        );
        assert!(TO_CONTROLLER.contains("set -- \"$@\" -c \"$ip\"\n\"$@\" &"));
        assert_eq!(
            tool_argv(&server()),
            [
                "iperf3",
                "-s",
                "-1",
                "-p",
                "5201",
                "-f",
                "m",
                "--forceflush"
            ]
        );
    }
}
