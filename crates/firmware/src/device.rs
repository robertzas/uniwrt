//! A target's devices in one release (pure; M7.1a1): the download server's
//! `releases/<release>/targets/<target>/profiles.json`, kept compact (the target's defaults
//! once, each device's own packages beside them), and the device a board is.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Error, catalog::valid_profile};

/// A target's devices in one release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The package architecture, such as `aarch64_cortex-a53`.
    pub(crate) arch: String,
    defaults: Vec<String>,
    devices: Vec<Device>,
}

/// One device: an image builder profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Device {
    pub(crate) id: String,
    /// The boards it builds for (`glinet,gl-mt6000`), old names included.
    boards: Vec<String>,
    /// What it adds to the target's defaults, or removes (`-name`).
    own: Vec<String>,
    /// The sysupgrade image's size.
    pub(crate) sysupgrade_bytes: Option<u64>,
}

#[derive(Deserialize)]
struct Raw {
    arch_packages: String,
    #[serde(default)]
    default_packages: Vec<String>,
    profiles: BTreeMap<String, RawDevice>,
}

#[derive(Deserialize)]
struct RawDevice {
    #[serde(default)]
    device_packages: Vec<String>,
    #[serde(default)]
    supported_devices: Vec<String>,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawImage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: Option<u64>,
}

pub(crate) fn target(body: &[u8]) -> Result<Target, Error> {
    let raw: Raw = serde_json::from_slice(body)
        .map_err(|err| Error::Invalid(format!("a target's profiles are not as expected: {err}")))?;
    if !valid_arch(&raw.arch_packages) {
        return Err(Error::Invalid(format!(
            "{} is not a package architecture",
            raw.arch_packages
        )));
    }
    let devices = raw
        .profiles
        .into_iter()
        .filter(|(id, _)| valid_profile(id))
        .map(|(id, device)| Device {
            sysupgrade_bytes: device
                .images
                .iter()
                .find(|image| image.kind == "sysupgrade")
                .and_then(|image| image.size),
            id,
            boards: device.supported_devices,
            own: device.device_packages,
        })
        .collect();
    Ok(Target {
        arch: raw.arch_packages,
        defaults: raw.default_packages,
        devices,
    })
}

impl Target {
    /// The device `board` is: the one that lists it, else the profile named like it
    /// (`glinet,gl-mt6000` is `glinet_gl-mt6000`), else the target's only one when it lists
    /// no board (x86's `generic`).
    pub(crate) fn device(&self, board: &str) -> Option<&Device> {
        let named = board.replacen(',', "_", 1);
        self.devices
            .iter()
            .find(|device| device.boards.iter().any(|known| known == board))
            .or_else(|| self.devices.iter().find(|device| device.id == named))
            .or(match self.devices.as_slice() {
                [only] if only.boards.is_empty() => Some(only),
                _ => None,
            })
    }

    /// What `device`'s image holds without asking.
    pub(crate) fn packages(&self, device: &Device) -> BTreeSet<String> {
        let mut packages: BTreeSet<String> = self.defaults.iter().cloned().collect();
        for package in &device.own {
            match package.strip_prefix('-') {
                Some(removed) => {
                    packages.remove(removed);
                }
                None => {
                    packages.insert(package.clone());
                }
            }
        }
        packages
    }
}

/// The package names of an ASU index: an architecture's (`packages/<arch>-index.json`, names to
/// versions) or a target's (`targets/<target>/index.json`, the same under `packages`).
pub(crate) fn index(body: &[u8]) -> Result<BTreeSet<String>, Error> {
    let raw: BTreeMap<String, serde_json::Value> = serde_json::from_slice(body)
        .map_err(|err| Error::Invalid(format!("a package index is not as expected: {err}")))?;
    let names = match raw.get("packages") {
        Some(serde_json::Value::Object(packages)) => packages.keys().cloned().collect(),
        _ => raw.into_keys().collect(),
    };
    Ok(names)
}

/// A package architecture as it goes into a URL (`aarch64_cortex-a53`, `x86_64`).
pub(crate) fn valid_arch(arch: &str) -> bool {
    !arch.is_empty()
        && arch
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILOGIC: &[u8] = br#"{"version_code":"r33311-981c978eff","arch_packages":"aarch64_cortex-a53",
        "default_packages":["dnsmasq","ppp","wpad-basic-mbedtls"],
        "profiles":{
          "glinet_gl-mt6000":{"device_packages":["kmod-mt7915e","-ppp"],
            "supported_devices":["glinet,gl-mt6000"],
            "images":[{"type":"factory","size":3},{"type":"sysupgrade","size":10598720}]},
          "bananapi_bpi-r4":{"device_packages":[],"supported_devices":["bananapi,bpi-r4","sinovoip,bpi-r4"],"images":[]},
          "../evil":{"supported_devices":["evil"]}}}"#;

    #[test]
    fn a_board_is_found_by_the_devices_it_lists() {
        let target = target(FILOGIC).unwrap();
        assert_eq!(target.arch, "aarch64_cortex-a53");
        let device = target.device("glinet,gl-mt6000").unwrap();
        assert_eq!(device.id, "glinet_gl-mt6000");
        assert_eq!(device.sysupgrade_bytes, Some(10_598_720));
        assert_eq!(
            target.packages(device).into_iter().collect::<Vec<_>>(),
            ["dnsmasq", "kmod-mt7915e", "wpad-basic-mbedtls"]
        );
        assert_eq!(
            target.device("sinovoip,bpi-r4").unwrap().id,
            "bananapi_bpi-r4"
        );
        assert!(target.device("evil").is_none());
        assert!(target.device("acme,unknown").is_none());
    }

    #[test]
    fn a_board_is_found_by_its_profiles_name_or_as_the_only_device() {
        let body = br#"{"arch_packages":"x86_64","profiles":{"generic":{"supported_devices":[]}}}"#;
        let x86 = target(body).unwrap();
        assert_eq!(x86.device("qemu-standard-pc-q35").unwrap().id, "generic");
        let body = br#"{"arch_packages":"mips_24kc","profiles":{
            "tplink_archer-c7-v5":{},"tplink_archer-c7-v2":{}}}"#;
        let ath79 = target(body).unwrap();
        assert_eq!(
            ath79.device("tplink,archer-c7-v5").unwrap().id,
            "tplink_archer-c7-v5"
        );
        assert!(ath79.device("tplink,archer-c6-v2").is_none());
    }

    #[test]
    fn indexes_list_names_flat_or_under_packages() {
        let arch = index(br#"{"luci":"26.1","wpad-mbedtls":"2025-r1"}"#).unwrap();
        assert_eq!(
            arch.into_iter().collect::<Vec<_>>(),
            ["luci", "wpad-mbedtls"]
        );
        let target =
            index(br#"{"architecture":"aarch64_cortex-a53","packages":{"kmod-batman-adv":"x"}}"#)
                .unwrap();
        assert_eq!(target.into_iter().collect::<Vec<_>>(), ["kmod-batman-adv"]);
    }

    #[test]
    fn an_architecture_that_cant_be_a_path_is_refused() {
        let body = br#"{"arch_packages":"../x","profiles":{}}"#;
        assert!(matches!(target(body), Err(Error::Invalid(_))));
    }
}
