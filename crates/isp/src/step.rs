//! The connectivity test's steps (features/wan.md, Connectivity test) and what to say when one
//! fails.

use serde::{Deserialize, Serialize};
use uniwrt_model::site::gateway::{WanConfig, WanIpv6, WanProto};

use crate::Preset;

/// A step of the connectivity test, in the order it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cfg_attr(feature = "ts", ts(rename = "WanStep"))]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// The WAN port has a link (and the VLAN device is up).
    Link,
    /// A DHCP lease, a PPP session, the static address or the DS-Lite tunnel.
    Address,
    /// A default route, and the ISP's gateway answers a ping.
    Gateway,
    /// Names resolve through the ISP's resolver and a public one.
    Dns,
    /// An HTTPS site answers.
    Https,
    /// A delegated prefix or the 6rd tunnel, and an IPv6 host answers a ping.
    Ipv6,
}

impl Step {
    pub const ALL: [Self; 6] = [
        Self::Link,
        Self::Address,
        Self::Gateway,
        Self::Dns,
        Self::Https,
        Self::Ipv6,
    ];
}

/// What to tell the user when `step` fails on `wan`: the preset's own hint, else one from the
/// WAN's settings and netifd's error codes on the interface (`errors`, such as `AUTH_FAILED`).
pub fn hint(step: Step, wan: &WanConfig, preset: Option<&Preset>, errors: &[String]) -> String {
    if let Some(hint) = preset.and_then(|p| p.hints.get(&step)) {
        return hint.clone();
    }
    let on_vlan = wan
        .vlan
        .map_or_else(String::new, |vid| format!(" on VLAN {vid}"));
    let has = |code: &str| errors.iter().any(|e| e == code);
    match step {
        Step::Link => "The WAN port has no link. Is the cable in the WAN port, and the modem or \
                       ONT on?"
            .to_owned(),
        Step::Address => match &wan.proto {
            WanProto::Dhcp(_) => format!(
                "No DHCP offer{on_vlan}. Is the modem or ONT in bridge mode{}?",
                if wan.vlan.is_some() {
                    ", and is the VLAN the ISP's"
                } else {
                    ", or does the ISP need a VLAN or a cloned MAC address"
                }
            ),
            WanProto::Pppoe { .. } if has("AUTH_FAILED") => {
                "The ISP refused the PPPoE username or password.".to_owned()
            }
            WanProto::Pppoe { .. } => format!(
                "No PPPoE server answered{on_vlan}. Is the modem in bridge mode, and does the \
                 ISP need a VLAN or a service name?"
            ),
            WanProto::Static { .. } => {
                "The static address didn't come up. Check the address and its prefix length."
                    .to_owned()
            }
            WanProto::DsLite { .. } => {
                "The DS-Lite tunnel didn't come up: DHCPv6 announced no AFTR, or the AFTR \
                 doesn't answer."
                    .to_owned()
            }
            WanProto::None => "IPv6 didn't come up, so the WAN has no address.".to_owned(),
        },
        Step::Gateway => match &wan.proto {
            WanProto::Static { .. } => {
                "The ISP's gateway doesn't answer. Check the gateway address.".to_owned()
            }
            _ => "The ISP's gateway doesn't answer a ping; the line may be down upstream, or \
                  block pings."
                .to_owned(),
        },
        Step::Dns => match &wan.proto {
            WanProto::Static { dns, .. } if dns.is_empty() => {
                "No DNS server is set: add the ISP's.".to_owned()
            }
            _ => "Names don't resolve through the ISP's DNS server or a public one.".to_owned(),
        },
        Step::Https => "HTTPS sites don't answer, although names resolve: the ISP may need a \
                        captive portal signed in, or the clock may be wrong."
            .to_owned(),
        Step::Ipv6 => match &wan.ipv6 {
            WanIpv6::SixRd { .. } => "The 6rd tunnel didn't carry traffic. Check the prefix, \
                                      the border relay and the IPv4 mask."
                .to_owned(),
            _ => "No IPv6 prefix was delegated. The ISP may offer no IPv6 here, or another way \
                  (6rd); IPv4 still works."
                .to_owned(),
        },
    }
}
