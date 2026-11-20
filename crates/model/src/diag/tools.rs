//! The tools of the diagnostics toolkit that OpenWrt's images lack (D-141): `tcpdump` for packet
//! captures, `iperf3` for throughput tests and `ethtool-full` for cable tests. On a managed node
//! the package job (D-139) installs [`OptionalTool::package`]; on a node in observe mode a
//! missing tool is unavailable.

use serde::{Deserialize, Serialize};

/// A tool OpenWrt's images lack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OptionalTool {
    /// Packet captures.
    Tcpdump,
    /// Throughput tests.
    Iperf3,
    /// Cable tests: the netlink build of ethtool (the plain `ethtool` package lacks them).
    Ethtool,
}

impl OptionalTool {
    pub const ALL: [Self; 3] = [Self::Tcpdump, Self::Iperf3, Self::Ethtool];

    /// The package installed for it: the smallest that does the job (`tcpdump-mini` writes
    /// captures as `tcpdump` does, without its printers).
    pub const fn package(self) -> &'static str {
        match self {
            Self::Tcpdump => "tcpdump-mini",
            Self::Iperf3 => "iperf3",
            Self::Ethtool => "ethtool-full",
        }
    }

    /// The packages that provide it, any one of which will do.
    pub const fn provided_by(self) -> &'static [&'static str] {
        match self {
            Self::Tcpdump => &["tcpdump", "tcpdump-mini"],
            Self::Iperf3 => &["iperf3", "iperf3-ssl"],
            Self::Ethtool => &["ethtool-full"],
        }
    }

    /// The installed package that installing [`Self::package`] must remove: the plain
    /// `ethtool`, whose files `ethtool-full` replaces.
    pub const fn conflicts(self) -> &'static [&'static str] {
        match self {
            Self::Ethtool => &["ethtool"],
            Self::Tcpdump | Self::Iperf3 => &[],
        }
    }

    /// Its status on a node whose installed packages answer `installed`.
    pub fn status(self, installed: impl Fn(&str) -> bool) -> OptionalToolStatus {
        OptionalToolStatus {
            tool: self,
            installed: self.provided_by().iter().any(|name| installed(name)),
            package: self.package().to_owned(),
            replaces: self
                .conflicts()
                .iter()
                .find(|name| installed(name))
                .map(|name| (*name).to_owned()),
        }
    }
}

/// Whether a node has an optional tool, and what installs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct OptionalToolStatus {
    pub tool: OptionalTool,
    pub installed: bool,
    /// The package to install for it.
    pub package: String,
    /// An installed package that installing it removes (the package job's `remove`).
    pub replaces: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::OptionalTool;

    #[test]
    fn a_tool_is_there_when_any_of_its_packages_is() {
        let status = OptionalTool::Tcpdump.status(|name| name == "tcpdump");
        assert!(status.installed);
        assert_eq!(status.package, "tcpdump-mini");
        let status = OptionalTool::Ethtool.status(|name| name == "ethtool");
        assert!(!status.installed, "the plain build has no cable test");
        assert_eq!(status.replaces.as_deref(), Some("ethtool"));
        let status = OptionalTool::Iperf3.status(|_| false);
        assert_eq!((status.installed, status.replaces), (false, None));
    }
}
