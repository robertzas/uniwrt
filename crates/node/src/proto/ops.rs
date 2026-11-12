//! The agent's operations: their names, arguments and results, mirrored by `node-agent/ops.uc`.
//! Send them with [`NodeLink::request`](crate::NodeLink::request); a failed one is
//! [`Error::Remote`](crate::Error::Remote) with one of the [`code`]s. The ops that make the agent
//! push events and ticks are in `streams.rs` and re-exported here.

mod btm;
mod mtu;
mod rrm;
mod scan;
mod streams;
mod usteer;

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, value::RawValue};
use uniwrt_core::Timestamp;

pub use self::btm::{Btm, BtmSent, MboReason, candidate};
pub use self::mtu::{Mtu, MtuProbe};
pub use self::rrm::{NeighborReport, OwnReport, Rrm, RrmFailure, RrmState};
pub use self::scan::{RawBss, RawSurvey, Scan, Scanned};
pub use self::streams::{
    LogFollow, LogFollowing, LogMode, Schedule, SetInterval, Subscribe, Subscribed, Topic,
    Unsubscribe,
};
pub use self::usteer::{HeardAt, client_info as usteer_client_info, heard as usteer_heard};
use super::RemoteError;

/// An operation: its name on the wire and the type of its result.
pub trait Op: Serialize + Sync {
    const NAME: &'static str;
    type Output: DeserializeOwned + Send;
}

/// Error codes of failed requests ([`RemoteError::code`]).
pub mod code {
    /// Unknown op, set or malformed arguments.
    pub const BAD_REQUEST: &str = "bad_request";
    /// No such file, ubus object or method.
    pub const NOT_FOUND: &str = "not_found";
    pub const TIMEOUT: &str = "timeout";
    /// A command wrote more output than allowed.
    pub const TOO_LARGE: &str = "too_large";
    pub const DENIED: &str = "denied";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const FAILED: &str = "failed";
}

/// `ping`: a round trip that also reads the node's clock.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Ping {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Pong {
    /// The node's clock.
    pub ts: Timestamp,
}

impl Op for Ping {
    const NAME: &'static str = "ping";
    type Output = Pong;
}

/// `collect`: the raw sources of parts of the node's facts, which [`facts`](crate::facts)
/// parses. Each set is collected on its own: one that fails is reported in
/// [`Collected::errors`] and the others still arrive.
#[derive(Debug, Clone, Serialize)]
pub struct Collect {
    pub sets: Vec<Set>,
}

impl Collect {
    pub fn all() -> Self {
        Self {
            sets: Set::ALL.to_vec(),
        }
    }
}

impl Op for Collect {
    const NAME: &'static str = "collect";
    type Output = Collected;
}

/// A collect set: one file in `node-agent/collect/` and one parser in [`facts`](crate::facts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Set {
    /// batman-adv meshes: originators, neighbors, gateways, translation tables.
    Batman,
    /// What the node is: model, firmware release, kernel.
    Board,
    /// Detected features: package manager, optional software, hostapd build, switch kind.
    Capabilities,
    /// The SHA-256 of each UCI package in `/etc/config`, for drift detection.
    Config,
    /// Kernel network devices with their counters.
    Devices,
    /// netifd's logical interfaces.
    Interfaces,
    /// DHCP leases (dnsmasq, odhcpd).
    Leases,
    /// The ARP and NDP caches and the bridges' forwarding databases.
    Neighbors,
    /// Radio hardware: bands, standards, widths, channels. Large and rarely changing, so it is
    /// collected seldom.
    Phys,
    /// Radios and their interfaces: configuration, channel, power, noise, utilization.
    Radios,
    /// Wi-Fi clients and mesh peers: signal, rates, counters, capabilities.
    Stations,
    /// Resource usage, the node's clock and temperatures.
    System,
}

impl Set {
    pub const ALL: [Self; 12] = [
        Self::Batman,
        Self::Board,
        Self::Capabilities,
        Self::Config,
        Self::Devices,
        Self::Interfaces,
        Self::Leases,
        Self::Neighbors,
        Self::Phys,
        Self::Radios,
        Self::Stations,
        Self::System,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Batman => "batman",
            Self::Board => "board",
            Self::Capabilities => "capabilities",
            Self::Config => "config",
            Self::Devices => "devices",
            Self::Interfaces => "interfaces",
            Self::Leases => "leases",
            Self::Neighbors => "neighbors",
            Self::Phys => "phys",
            Self::Radios => "radios",
            Self::Stations => "stations",
            Self::System => "system",
        }
    }
}

impl fmt::Display for Set {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Deserialize)]
pub struct Collected {
    pub sets: BTreeMap<Set, Box<RawValue>>,
    #[serde(default)]
    pub errors: BTreeMap<Set, RemoteError>,
}

/// `ubus`: one call. The result is the method's reply, `null` when it sends none.
#[derive(Debug, Clone, Serialize)]
pub struct Ubus {
    pub path: String,
    pub method: String,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub data: Map<String, Value>,
}

impl Ubus {
    pub fn new(path: impl Into<String>, method: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            method: method.into(),
            data: Map::new(),
        }
    }
}

impl Op for Ubus {
    const NAME: &'static str = "ubus";
    type Output = Box<RawValue>;
}

/// `exec`: runs a command on the node. Its output goes to files in `/tmp/uniwrt/run`, never to
/// the agent's stdout; a command that outlives `timeout_ms` is killed ([`code::TIMEOUT`]), and
/// one that writes more than `max_output` bytes to stdout or stderr fails with
/// [`code::TOO_LARGE`].
#[derive(Debug, Clone, Serialize)]
pub struct Exec {
    pub argv: Vec<String>,
    pub timeout_ms: u64,
    pub max_output: u64,
}

impl Exec {
    pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
    pub const DEFAULT_MAX_OUTPUT: u64 = 1 << 20;

    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            timeout_ms: Self::DEFAULT_TIMEOUT_MS,
            max_output: Self::DEFAULT_MAX_OUTPUT,
        }
    }
}

impl Op for Exec {
    const NAME: &'static str = "exec";
    type Output = Executed;
}

/// How a command ended, as the shell reports it: an exit code, or the signal that killed it.
/// Bytes that are not UTF-8 arrive as U+FFFD.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Executed {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Executed {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// `read_file`: up to `max_bytes` of a file. Binary files need `base64`, since bytes that are
/// not UTF-8 arrive as U+FFFD.
#[derive(Debug, Clone, Serialize)]
pub struct ReadFile {
    pub path: String,
    pub max_bytes: u64,
    pub base64: bool,
}

impl ReadFile {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            max_bytes: 1 << 20,
            base64: false,
        }
    }
}

impl Op for ReadFile {
    const NAME: &'static str = "read_file";
    type Output = FileData;
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileData {
    pub data: String,
    /// The file is longer than `max_bytes`.
    pub truncated: bool,
}

/// `bss`: turns BSSes of a phy off and its others on, without touching UCI (`node-agent/bss.uc`;
/// D-083): hostapd takes netifd's config of the phy (of each radio, on a multi-radio phy) less the
/// BSSes in `off`, by interface name. With `off` empty all are on, which changes nothing when they
/// are. [`code::NOT_FOUND`] when netifd has no hostapd config of the phy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bss {
    pub phy: String,
    pub off: Vec<String>,
}

impl Op for Bss {
    const NAME: &'static str = "bss";
    type Output = BssSet;
}

/// What hostapd took, per config of the phy.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BssSet {
    pub radios: Vec<BssRadio>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BssRadio {
    /// The radio of a multi-radio phy; `None` for the phy's only one.
    pub radio: Option<u32>,
    /// The BSSes it runs, and those it left out.
    pub on: Vec<String>,
    pub off: Vec<String>,
}

/// `ban`: keeps clients off every access point of the node for a while, or lets them back
/// (`node-agent/ban.uc`; D-084), through hostapd's `del_client` on each BSS: a ban deauthenticates
/// the client where it is connected and has hostapd refuse it until the ban lapses or the BSS
/// restarts; a lift ([`BanEntry::ms`] 0) leaves the BSSes it is connected to alone, unless it
/// kicks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ban {
    pub bans: Vec<BanEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BanEntry {
    /// In lower case, as hostapd writes it.
    pub addr: String,
    /// How long, at most [`Ban::MAX_MS`]; 0 lifts it.
    pub ms: u32,
    /// Deauthenticates the client wherever it is connected, even with `ms` 0. A ban does
    /// anyway.
    pub kick: bool,
}

impl Ban {
    /// The longest ban: hostapd's timer takes it in microseconds in an `int`.
    pub const MAX_MS: u32 = 1_800_000;
    /// The most entries one request takes.
    pub const MAX_BANS: usize = 512;
}

impl Op for Ban {
    const NAME: &'static str = "ban";
    type Output = Banned;
}

/// What hostapd did: the BSSes it acted on, the clients it deauthenticated, and the calls that
/// failed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Banned {
    pub bsses: Vec<String>,
    pub kicked: Vec<BanKick>,
    pub failed: Vec<BanFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BanKick {
    pub ifname: String,
    pub addr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BanFailure {
    pub ifname: String,
    pub addr: String,
    pub error: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Collect, Collected, Exec, Ping, Set, Ubus};

    #[test]
    fn requests_serialize_as_the_agent_reads_them() {
        assert_eq!(serde_json::to_value(Ping {}).unwrap(), json!({}));
        assert_eq!(
            serde_json::to_value(Collect::all()).unwrap(),
            json!({"sets": [
                "batman", "board", "capabilities", "config", "devices", "interfaces", "leases", "neighbors",
                "phys", "radios", "stations", "system"
            ]})
        );
        assert_eq!(
            serde_json::to_value(Ubus::new("system", "board")).unwrap(),
            json!({"path": "system", "method": "board"})
        );
        assert_eq!(
            serde_json::to_value(Exec::new(["batctl", "o"])).unwrap(),
            json!({"argv": ["batctl", "o"], "timeout_ms": 10_000, "max_output": 1_048_576})
        );
    }

    #[test]
    fn set_names_match_serde() {
        for set in Set::ALL {
            assert_eq!(serde_json::to_value(set).unwrap(), json!(set.as_str()));
        }
    }

    #[test]
    fn collected_sets_and_errors_are_split() {
        let collected: Collected = serde_json::from_str(
            r#"{"sets":{"board":{"a":1}},"errors":{"system":{"code":"failed","msg":"no ubus"}}}"#,
        )
        .unwrap();
        assert_eq!(collected.sets[&Set::Board].get(), r#"{"a":1}"#);
        assert_eq!(collected.errors[&Set::System].msg, "no ubus");
    }
}
