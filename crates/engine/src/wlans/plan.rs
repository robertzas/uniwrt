//! Which of a node's BSSes the WLANs' schedules want on now: pure, over the saved site and the
//! node's facts, so a schedule needs no apply (it changes no configuration on the node).

use uniwrt_core::{NodeId, Timestamp};
use uniwrt_model::{
    facts::{RadioFacts, WifiIfaceMode},
    site::{SiteConfig, node::ManagedPackage, tz},
};

/// One BSS of a WLAN on a node, and whether its schedule has it on now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bss {
    pub(crate) wlan: String,
    /// Its `wifi-iface` section, such as `uw_guest_2g`.
    pub(crate) section: String,
    pub(crate) radio: String,
    /// The radio's phy, which the `bss` op names; unknown while a radio is down.
    pub(crate) phy: Option<String>,
    /// Whether the radio is up: a BSS of a radio that is down isn't the schedule's to turn on.
    pub(crate) radio_up: bool,
    /// Its kernel device, unknown while it is down.
    pub(crate) ifname: Option<String>,
    /// Whether the node reports it up.
    pub(crate) up: bool,
    pub(crate) on: bool,
}

/// The BSSes of `node`'s enabled WLANs, as its radios report them, where the node manages
/// `wireless`; none elsewhere, so a node in observe mode is never touched. A WLAN without a
/// schedule is on, and so is every WLAN while the site's time zone can't be read: off only
/// when the schedule says so.
pub(crate) fn bsses(
    config: &SiteConfig,
    node: NodeId,
    radios: &[RadioFacts],
    now: Timestamp,
) -> Vec<Bss> {
    let manages = config
        .nodes
        .iter()
        .any(|n| n.node == node && n.managed.contains(&ManagedPackage::Wireless));
    if !manages {
        return Vec::new();
    }
    let local = tz::posix(&config.site.timezone).and_then(|posix| tz::local(posix, now));
    let mut out = Vec::new();
    for radio in radios {
        for iface in &radio.ifaces {
            let Some(section) = iface.section.as_deref() else {
                continue;
            };
            if iface.mode != WifiIfaceMode::Ap {
                continue;
            }
            let Some(wlan) = config
                .wlans
                .iter()
                .find(|w| w.enabled && w.owns_section(section))
            else {
                continue;
            };
            let on = match (&wlan.schedule, local) {
                (Some(schedule), Some(local)) => schedule.is_on(local.day, local.minute),
                _ => true,
            };
            out.push(Bss {
                wlan: wlan.id.clone(),
                section: section.to_owned(),
                radio: radio.name.clone(),
                phy: radio.phy.clone(),
                radio_up: radio.up,
                ifname: iface.name.clone(),
                up: iface.up,
                on,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use uniwrt_core::{NodeId, Timestamp};
    use uniwrt_model::{
        facts::{RadioFacts, WifiIfaceFacts, WifiIfaceMode},
        site::{
            SiteConfig,
            node::{ManagedPackage, NodeConfig},
            wireless::{NodeScope, Schedule, Span, Weekday, Wlan, WlanSecurity},
        },
        wifi::Band,
    };

    use super::{Bss, bsses};

    const NODE: NodeId = NodeId::new(7);

    fn iface(section: &str, name: &str, mode: WifiIfaceMode) -> WifiIfaceFacts {
        WifiIfaceFacts {
            name: Some(name.to_owned()),
            section: Some(section.to_owned()),
            mode,
            ssid: None,
            mesh_id: None,
            bssid: None,
            encryption: None,
            networks: Vec::new(),
            up: true,
        }
    }

    fn radio(name: &str, band: Band, ifaces: Vec<WifiIfaceFacts>) -> RadioFacts {
        RadioFacts {
            name: name.to_owned(),
            phy: None,
            path: None,
            band,
            up: true,
            disabled: false,
            restart_pending: false,
            channel: None,
            frequency_mhz: None,
            center_frequency_mhz: None,
            width: None,
            standard: None,
            tx_power_dbm: None,
            noise_dbm: None,
            channel_busy_percent: None,
            country: None,
            ifaces,
        }
    }

    /// The default site with `home` on all day and `guest` from 08:00 to 22:00 on weekdays,
    /// Berlin time, and the node managing `wireless`.
    fn site() -> SiteConfig {
        let mut config = SiteConfig::defaults(None);
        config.site.timezone = "Europe/Berlin".to_owned();
        let home = Wlan {
            id: "home".to_owned(),
            ssid: "Home".to_owned(),
            enabled: true,
            network: "lan".to_owned(),
            security: WlanSecurity::Open,
            passphrase: None,
            bands: vec![Band::G2, Band::G5],
            nodes: NodeScope::All,
            hidden: false,
            isolation: false,
            max_clients: None,
            schedule: None,
            roaming: uniwrt_model::site::wireless::Roaming::default(),
        };
        let mut guest = home.clone();
        guest.id = "guest".to_owned();
        guest.schedule = Some(Schedule {
            spans: vec![Span {
                days: Weekday::ALL[..5].to_vec(),
                from: 8 * 60,
                to: 22 * 60,
            }],
        });
        config.wlans = vec![home, guest];
        let mut node = NodeConfig::new(NODE);
        node.managed = BTreeSet::from([ManagedPackage::Wireless]);
        config.nodes = vec![node];
        config
    }

    fn radios() -> Vec<RadioFacts> {
        vec![
            radio(
                "radio0",
                Band::G2,
                vec![
                    iface("uw_home_2g", "phy0-ap0", WifiIfaceMode::Ap),
                    iface("uw_guest_2g", "phy0-ap1", WifiIfaceMode::Ap),
                    iface("uw_mesh", "phy0-mesh0", WifiIfaceMode::Mesh),
                ],
            ),
            radio(
                "radio1",
                Band::G5,
                vec![
                    iface("uw_guest_5g", "phy1-ap0", WifiIfaceMode::Ap),
                    iface("default_radio1", "phy1-ap1", WifiIfaceMode::Ap),
                ],
            ),
        ]
    }

    /// 2026-11-11 (a Wednesday) at `hh:mm` UTC; Berlin is an hour ahead.
    fn wednesday(hh: i64, mm: i64) -> Timestamp {
        Timestamp::from_unix_secs(1_794_355_200 + hh * 3_600 + mm * 60)
    }

    fn on(list: &[Bss]) -> Vec<(&str, bool)> {
        list.iter().map(|b| (b.section.as_str(), b.on)).collect()
    }

    #[test]
    fn the_schedule_turns_its_bsses_off_in_the_sites_time() {
        let config = site();
        // 21:59 in Berlin: still on; 22:00: off, on both radios; the other WLAN stays on,
        // and what isn't a managed WLAN's AP is left out.
        let before = bsses(&config, NODE, &radios(), wednesday(20, 59));
        assert_eq!(
            on(&before),
            [
                ("uw_home_2g", true),
                ("uw_guest_2g", true),
                ("uw_guest_5g", true)
            ]
        );
        let after = bsses(&config, NODE, &radios(), wednesday(21, 0));
        assert_eq!(
            on(&after),
            [
                ("uw_home_2g", true),
                ("uw_guest_2g", false),
                ("uw_guest_5g", false)
            ]
        );
        assert_eq!(after[1].radio, "radio0");
        assert_eq!(after[1].ifname.as_deref(), Some("phy0-ap1"));
        assert_eq!(after[1].wlan, "guest");
        // 07:00 UTC is 08:00 in Berlin.
        let morning = bsses(&config, NODE, &radios(), wednesday(7, 0));
        assert!(morning.iter().all(|b| b.on));
    }

    #[test]
    fn nothing_is_touched_without_management_and_nothing_goes_off_without_a_clock() {
        let night = wednesday(23, 0);
        let mut config = site();
        config.nodes[0].managed.clear();
        assert_eq!(bsses(&config, NODE, &radios(), night), []);
        assert_eq!(bsses(&site(), NodeId::new(8), &radios(), night), []);
        // A time zone tzdata doesn't know leaves every BSS on.
        let mut config = site();
        config.site.timezone = "Mars/Olympus".to_owned();
        assert!(bsses(&config, NODE, &radios(), night).iter().all(|b| b.on));
        // A disabled WLAN isn't the schedule's: render keeps it off.
        let mut config = site();
        config.wlans[1].enabled = false;
        assert_eq!(
            on(&bsses(&config, NODE, &radios(), night)),
            [("uw_home_2g", true)]
        );
    }
}
