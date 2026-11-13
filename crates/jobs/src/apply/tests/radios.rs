//! A fake node's radios for the Channels round (D-113): hostapd's `switch_chan` (taken, refused
//! or stuck), the kernel's channel the agent's `radios` set reports (nl80211's), and
//! `wifi reload`, which restarts the radios on what they were switched to.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uniwrt_node::{Error, ExecOutput, Result};

use super::fleet::{FakeNode, output};

/// The radios of hwsim-b on 24.10 (`crates/node/fixtures/collect`): `radio0` on 5 GHz carries
/// the mesh beside its access point, `radio1` on 2.4 GHz (`phy4-ap0`) runs channel 11.
const HWSIM_B: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../node/fixtures/collect/hwsim-b-24.10.8.json"
));

/// How hostapd answers `switch_chan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Switching {
    /// The kernel runs the new channel at once.
    #[default]
    Takes,
    /// hostapd refuses it (a DFS channel without CAC, a driver without CSA).
    Refused,
    /// hostapd accepts it, and the kernel stays where it was (the hwsim lab: no beacons).
    Stuck,
}

#[derive(Debug, Default)]
pub(crate) struct FakeRadios {
    pub(crate) answer: Switching,
    /// Each `switch_chan`, as `(ifname, arguments)`.
    pub(crate) switches: Vec<(String, Value)>,
    pub(crate) reloads: usize,
    /// The kernel's channel by interface: frequency, center and nl80211's width.
    running: BTreeMap<String, (u64, u64, u64)>,
}

impl FakeRadios {
    pub(crate) fn ubus(&mut self, args: &Value) -> Result<Value> {
        let path = args["path"].as_str().unwrap_or_default();
        let ifname = path.strip_prefix("hostapd.").unwrap_or_default().to_owned();
        assert_eq!(args["method"], "switch_chan", "{args}");
        if self.answer == Switching::Refused {
            return Err(Error::Remote {
                op: "ubus".to_owned(),
                code: "failed".to_owned(),
                msg: "Unknown error".to_owned(),
            });
        }
        self.switches.push((ifname.clone(), args["data"].clone()));
        if self.answer == Switching::Takes {
            self.run(&ifname, &args["data"]);
        }
        Ok(json!({}))
    }

    /// `wifi reload`: every radio switched restarts where the configuration puts it.
    pub(crate) fn reload(&mut self) -> ExecOutput {
        self.reloads += 1;
        for (ifname, data) in self.switches.clone() {
            self.run(&ifname, &data);
        }
        output(0, "")
    }

    fn run(&mut self, ifname: &str, data: &Value) {
        let width = match data["bandwidth"].as_u64() {
            Some(40) => 2,
            Some(80) => 3,
            Some(160) => 5,
            _ => 1,
        };
        let (freq, center) = (data["freq"].as_u64(), data["center_freq1"].as_u64());
        self.running.insert(
            ifname.to_owned(),
            (freq.unwrap_or(0), center.unwrap_or(0), width),
        );
    }

    /// Reports the kernel's channels in a collect.
    pub(crate) fn patch(&self, collected: &mut Value) {
        let Some(ifaces) = collected
            .pointer_mut("/sets/radios/interfaces")
            .and_then(Value::as_array_mut)
        else {
            return;
        };
        for iface in ifaces {
            let name = iface["ifname"].as_str().unwrap_or_default().to_owned();
            if let Some((freq, center, width)) = self.running.get(&name) {
                iface["wiphy_freq"] = json!(freq);
                iface["center_freq1"] = json!(center);
                iface["channel_width"] = json!(width);
            }
        }
    }
}

/// Gives `fake` hwsim-b's radios and phys, as Wi-Fi 6 (hwsim's Wi-Fi 7 reloads: `switch_chan`
/// has no EHT flag).
pub(crate) fn wifi(fake: &FakeNode) {
    let mut hwsim: Value = serde_json::from_str(HWSIM_B).unwrap();
    let wiphys = hwsim.pointer_mut("/sets/phys/wiphys").unwrap();
    for band in wiphys
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|w| w["bands"].as_array_mut().unwrap().iter_mut())
    {
        let datas = band.get_mut("iftype_data").and_then(Value::as_array_mut);
        for data in datas.into_iter().flatten() {
            data.as_object_mut()
                .unwrap()
                .retain(|key, _| !key.starts_with("eht"));
        }
    }
    *fake.collect_hook.lock().unwrap() = Some(Box::new(move |_, collected| {
        for set in ["radios", "phys"] {
            collected["sets"][set] = hwsim["sets"][set].clone();
        }
    }));
}
