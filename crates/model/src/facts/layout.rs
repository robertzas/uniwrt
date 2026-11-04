//! The board's Ethernet layout (`/etc/board.json`, which OpenWrt's own `config_generate` reads):
//! which ports form the LAN and the WAN, the swconfig switch and its CPU ports, and the MAC
//! addresses the board assigns. Render builds the network package on it.

use serde::{Deserialize, Serialize};
use uniwrt_core::MacAddr;

/// What board.json says about the board's network hardware.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BoardLayout {
    /// The LAN's ports (`network.lan.ports`, else its `device`): DSA user ports such as `lan1`,
    /// plain NICs such as `eth0`, or on a swconfig board the CPU port's VLAN device (`eth0.1`).
    pub lan: Vec<String>,
    /// The MAC address board.json assigns the LAN.
    pub lan_mac: Option<MacAddr>,
    /// The WAN's device (`network.wan.device`, else its first port), such as `wan`, `eth1` or
    /// `eth0.2`; `None` on boards without a WAN.
    pub wan: Option<String>,
    pub wan_mac: Option<MacAddr>,
    /// The swconfig switch; `None` without one.
    pub switch: Option<SwitchLayout>,
    /// `system.compat_version`, which sysupgrade compares with an image's.
    pub compat_version: Option<String>,
}

/// A switch configured through swconfig (`switch_vlan` sections).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SwitchLayout {
    /// The switch's name, such as `switch0`.
    pub name: String,
    /// The user ports of the LAN, by switch port number, in board order.
    pub lan_ports: Vec<u8>,
    /// The user ports of the WAN, when the WAN is on the switch.
    pub wan_ports: Vec<u8>,
    /// The CPU port the LAN reaches the SoC through.
    pub lan_cpu: Option<CpuPort>,
    /// The CPU port the WAN reaches the SoC through (another one on some boards).
    pub wan_cpu: Option<CpuPort>,
}

/// A switch port wired to the SoC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CpuPort {
    pub num: u8,
    /// The SoC's network device behind it, such as `eth0`.
    pub device: String,
}

impl SwitchLayout {
    /// The CPU ports in use, without repeats.
    pub fn cpu_ports(&self) -> Vec<&CpuPort> {
        let mut ports: Vec<&CpuPort> = Vec::new();
        for port in [&self.lan_cpu, &self.wan_cpu].into_iter().flatten() {
            if !ports.iter().any(|p| p.num == port.num) {
                ports.push(port);
            }
        }
        ports
    }
}
