//! The preset format (features/wan.md, Preset format) and how a preset and the user's answers
//! become the site's WAN.

use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};
use uniwrt_core::{Ipv4Net, Ipv6Net, MacAddr};
use uniwrt_model::site::{
    SecretRef,
    gateway::{DhcpClient, WanConfig, WanIpv6, WanProto},
};

use crate::Step;

/// One ISP connection variant, such as Lumen's IPoE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// The file name without `.toml`: `<country>-<isp>-<variant>`, or `generic-<kind>`.
    pub id: String,
    /// The ISP, or what the generic preset suits, such as "Cable and most fiber".
    pub name: String,
    /// The connection variant, such as "IPoE" or "PPPoE".
    pub variant: String,
    /// ISO 3166-1 alpha-2; unset for the generic presets every country offers.
    #[serde(default)]
    pub country: Option<String>,
    pub status: Status,
    /// Where the values are confirmed.
    #[serde(default)]
    pub sources: Vec<String>,
    /// What to know before switching, such as putting the ONT in bridge mode.
    #[serde(default)]
    pub notes: Option<String>,
    /// The fields the user fills in, in the order the wizard asks for them.
    #[serde(default)]
    pub prompts: Vec<Prompt>,
    pub ipv4: PresetIpv4,
    pub ipv6: PresetIpv6,
    /// What to say when a step of the connectivity test fails, before the generic hint.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(as = "std::collections::HashMap<Step, String>"))]
    pub hints: std::collections::BTreeMap<Step, String>,
}

/// How sure the values are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "ts", ts(rename = "PresetStatus"))]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Confirmed by the ISP's own documentation or on a uniwrt installation.
    Verified,
    /// Confirmed by users' reports.
    Community,
    Unverified,
}

/// A field the user fills in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Prompt {
    /// PPPoE's username and password.
    Username,
    Password,
    /// A static address with its prefix, the ISP's gateway and DNS servers.
    Address,
    Gateway,
    Dns,
    /// The ISP's VLAN.
    Vlan,
    /// A MAC address to clone, such as the ISP router's.
    Mac,
    /// DS-Lite's AFTR.
    Aftr,
    /// 6rd's prefix, border relay and IPv4 mask.
    Sixrd,
}

/// How IPv4 arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct PresetIpv4 {
    pub proto: Ipv4Kind,
    #[serde(default)]
    pub vlan: Option<u16>,
    #[serde(default)]
    pub pcp: Option<u8>,
    #[serde(default)]
    pub mtu: Option<u16>,
    /// What the DHCP client sends.
    #[serde(default)]
    pub dhcp: Option<PresetDhcp>,
    /// PPPoE's service name.
    #[serde(default)]
    pub service: Option<String>,
    /// DS-Lite's AFTR; unset, DHCPv6 announces it.
    #[serde(default)]
    pub aftr: Option<String>,
}

/// What the DHCP client sends, as a preset spells it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct PresetDhcp {
    #[serde(default)]
    pub vendor_class: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub send_options: Vec<String>,
    #[serde(default)]
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Ipv4Kind {
    Dhcp,
    Pppoe,
    Static,
    Dslite,
    /// IPv6 only.
    None,
}

/// How IPv6 arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct PresetIpv6 {
    pub mode: Ipv6Kind,
    /// The prefix length DHCPv6 asks for; unset, whatever the ISP delegates.
    #[serde(default)]
    pub reqprefix: Option<u8>,
    /// 6rd's values, for `6rd` and `auto`; `6rd` without them prompts for them.
    #[serde(default)]
    pub sixrd: Option<SixRd>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Ipv6Kind {
    Off,
    Dhcpv6,
    #[serde(rename = "6rd")]
    SixRd,
    /// DHCPv6-PD, else 6rd: the connectivity test decides.
    Auto,
}

/// 6rd's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(deny_unknown_fields)]
pub struct SixRd {
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub prefix: Ipv6Net,
    #[cfg_attr(feature = "ts", ts(as = "String"))]
    pub border_relay: Ipv4Addr,
    #[serde(default)]
    pub ipv4_mask_len: u8,
}

impl From<SixRd> for WanIpv6 {
    fn from(sixrd: SixRd) -> Self {
        Self::SixRd {
            prefix: sixrd.prefix,
            border_relay: sixrd.border_relay,
            ipv4_mask_len: sixrd.ipv4_mask_len,
        }
    }
}

/// What the user filled in for a preset's [`Prompt`]s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answers {
    pub username: Option<String>,
    pub password: Option<SecretRef>,
    pub address: Option<Ipv4Net>,
    pub gateway: Option<Ipv4Addr>,
    pub dns: Vec<IpAddr>,
    pub vlan: Option<u16>,
    pub mac: Option<MacAddr>,
    pub aftr: Option<String>,
    pub sixrd: Option<SixRd>,
}

impl Preset {
    /// The site's WAN from this preset and `answers`, or the first prompt a required value is
    /// missing for. IPv6 `auto` starts with DHCPv6 ([`Preset::fallback`] has the 6rd).
    pub fn wan(&self, answers: &Answers) -> Result<WanConfig, Prompt> {
        let v4 = &self.ipv4;
        let proto = match v4.proto {
            Ipv4Kind::Dhcp => {
                let dhcp = v4.dhcp.clone().unwrap_or_default();
                WanProto::Dhcp(DhcpClient {
                    vendor_class: dhcp.vendor_class,
                    client_id: dhcp.client_id,
                    send_options: dhcp.send_options,
                    hostname: dhcp.hostname,
                })
            }
            Ipv4Kind::Pppoe => WanProto::Pppoe {
                username: answers.username.clone().ok_or(Prompt::Username)?,
                password: Some(answers.password.clone().ok_or(Prompt::Password)?),
                service: v4.service.clone(),
            },
            Ipv4Kind::Static => WanProto::Static {
                address: answers.address.ok_or(Prompt::Address)?,
                gateway: answers.gateway.ok_or(Prompt::Gateway)?,
                dns: answers.dns.clone(),
            },
            Ipv4Kind::Dslite => WanProto::DsLite {
                aftr: answers.aftr.clone().or_else(|| v4.aftr.clone()),
            },
            Ipv4Kind::None => WanProto::None,
        };
        let v6 = &self.ipv6;
        let ipv6 = match v6.mode {
            Ipv6Kind::Off => WanIpv6::Off,
            Ipv6Kind::Dhcpv6 | Ipv6Kind::Auto => WanIpv6::Dhcpv6 {
                reqprefix: v6.reqprefix,
            },
            Ipv6Kind::SixRd => v6.sixrd.or(answers.sixrd).ok_or(Prompt::Sixrd)?.into(),
        };
        Ok(WanConfig {
            proto,
            vlan: answers.vlan.or(v4.vlan),
            pcp: v4.pcp,
            mtu: v4.mtu,
            mac: answers.mac,
            ipv6,
            preset: Some(self.id.clone()),
        })
    }

    /// The IPv6 to try when DHCPv6 delegates no prefix: `auto`'s 6rd.
    pub fn fallback(&self) -> Option<WanIpv6> {
        match self.ipv6.mode {
            Ipv6Kind::Auto => self.ipv6.sixrd.map(Into::into),
            _ => None,
        }
    }
}
