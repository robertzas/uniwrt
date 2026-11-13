//! OpenWrt's catalog, as ASU and OpenWrt's download server describe it (pure): the releases a
//! zero-touch image can be built for, the devices of a release, and one device's packages.

use std::collections::BTreeSet;

use serde::Deserialize;
use uniwrt_model::firmware::FirmwareProfile;

use crate::Error;

/// The oldest branch uniwrt adopts (features/adoption.md, Path A).
const OLDEST: (u32, u32) = (24, 10);

/// ASU's `json/v1/overview.json`: its branches' latest releases, and the download server it
/// builds from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Overview {
    /// The latest release of each supported branch, newest first.
    pub(crate) releases: Vec<String>,
    /// The download server, such as `https://downloads.openwrt.org`, without a trailing `/`.
    pub(crate) upstream: String,
}

#[derive(Deserialize)]
struct RawOverview {
    latest: Vec<String>,
    upstream_url: String,
}

pub(crate) fn overview(body: &[u8]) -> Result<Overview, Error> {
    let raw: RawOverview = serde_json::from_slice(body).map_err(|err| invalid("overview", &err))?;
    let mut releases: Vec<((u32, u32, u32), String)> = raw
        .latest
        .into_iter()
        .filter_map(|release| Some((version(&release)?, release)))
        .filter(|((major, minor, _), _)| (*major, *minor) >= OLDEST)
        .collect();
    releases.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    releases.dedup_by(|a, b| a.0 == b.0);
    Ok(Overview {
        releases: releases.into_iter().map(|(_, release)| release).collect(),
        upstream: raw.upstream_url.trim_end_matches('/').to_owned(),
    })
}

/// A final release's number (`25.12.6`): release candidates and snapshots have none.
fn version(release: &str) -> Option<(u32, u32, u32)> {
    let mut parts = release.split('.').map(|part| part.parse::<u32>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

#[derive(Deserialize)]
struct RawProfiles {
    profiles: Vec<RawProfile>,
}

#[derive(Deserialize)]
struct RawProfile {
    id: String,
    target: String,
    #[serde(default)]
    titles: Vec<Title>,
}

#[derive(Deserialize)]
struct Title {
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    variant: Option<String>,
    /// A title of its own, which some profiles give instead (`Generic x86/64`).
    #[serde(default, rename = "title")]
    whole: Option<String>,
}

impl Title {
    fn text(&self) -> Option<String> {
        if let Some(title) = self
            .whole
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            return Some(title.trim().to_owned());
        }
        let words: Vec<&str> = [&self.vendor, &self.model, &self.variant]
            .into_iter()
            .filter_map(|word| {
                word.as_deref()
                    .map(str::trim)
                    .filter(|word| !word.is_empty())
            })
            .collect();
        (!words.is_empty()).then(|| words.join(" "))
    }
}

/// A release's devices (`releases/<release>/.overview.json` on the download server), sorted by
/// their first title. A profile without a title is named by its ID.
pub(crate) fn profiles(body: &[u8]) -> Result<Vec<FirmwareProfile>, Error> {
    let raw: RawProfiles = serde_json::from_slice(body).map_err(|err| invalid("profiles", &err))?;
    let mut profiles: Vec<FirmwareProfile> = raw
        .profiles
        .into_iter()
        .filter(|profile| valid_target(&profile.target) && valid_profile(&profile.id))
        .map(|profile| {
            let mut titles: Vec<String> = profile.titles.iter().filter_map(Title::text).collect();
            if titles.is_empty() {
                titles.push(profile.id.clone());
            }
            FirmwareProfile {
                id: profile.id,
                target: profile.target,
                titles,
            }
        })
        .collect();
    profiles.sort_by_cached_key(|profile| (profile.titles[0].to_lowercase(), profile.id.clone()));
    Ok(profiles)
}

/// One device's packages in a release (ASU's `json/v1/releases/<release>/targets/<target>/<profile>.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileInfo {
    /// What its image holds without asking: the target's defaults with the device's own added
    /// and those it removes (`-name`) taken out.
    pub(crate) packages: BTreeSet<String>,
}

#[derive(Deserialize)]
struct RawInfo {
    #[serde(default)]
    default_packages: Vec<String>,
    #[serde(default)]
    device_packages: Vec<String>,
}

pub(crate) fn profile_info(body: &[u8]) -> Result<ProfileInfo, Error> {
    let raw: RawInfo = serde_json::from_slice(body).map_err(|err| invalid("profile", &err))?;
    let mut packages: BTreeSet<String> = raw.default_packages.into_iter().collect();
    for package in raw.device_packages {
        match package.strip_prefix('-') {
            Some(removed) => {
                packages.remove(removed);
            }
            None => {
                packages.insert(package);
            }
        }
    }
    Ok(ProfileInfo { packages })
}

/// A target and subtarget as ASU takes them (`mediatek/filogic`): they become a URL's path.
pub(crate) fn valid_target(target: &str) -> bool {
    target.split_once('/').is_some_and(|(target, subtarget)| {
        [target, subtarget].iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    })
}

/// A profile's ID as ASU takes it (`glinet_gl-mt6000`).
pub(crate) fn valid_profile(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b','))
}

fn invalid(what: &str, err: &serde_json::Error) -> Error {
    Error::Invalid(format!("ASU's {what} is not as expected: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overview_keeps_each_supported_branchs_latest_newest_first() {
        let body = br#"{"latest":["24.10.8","25.12.6","23.05.6","25.12.0-rc5","SNAPSHOT"],
            "upstream_url":"https://downloads.openwrt.org/","branches":{}}"#;
        assert_eq!(
            overview(body).unwrap(),
            Overview {
                releases: vec!["25.12.6".to_owned(), "24.10.8".to_owned()],
                upstream: "https://downloads.openwrt.org".to_owned(),
            }
        );
        assert!(matches!(overview(b"{}"), Err(Error::Invalid(_))));
    }

    #[test]
    fn profiles_are_named_by_their_titles_and_sorted() {
        let body = br#"{"release":"25.12.6","profiles":[
            {"id":"glinet_gl-mt6000","titles":[{"model":"GL-MT6000","vendor":"GL.iNet"}],"target":"mediatek/filogic"},
            {"id":"generic","titles":[{"title":"Generic x86/64"}],"target":"x86/64"},
            {"id":"ctera_c200-v1","titles":[{"model":"C200","variant":"V1","vendor":"Ctera"},{"model":"C200 V1b","vendor":"Ctera"}],"target":"kirkwood/generic"},
            {"id":"untitled","titles":[],"target":"ath79/generic"},
            {"id":"../evil","titles":[],"target":"ath79/generic"},
            {"id":"bad-target","titles":[],"target":"ath79/../x"}]}"#;
        let profiles = profiles(body).unwrap();
        let named: Vec<(&str, Vec<&str>)> = profiles
            .iter()
            .map(|p| (p.id.as_str(), p.titles.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(
            named,
            [
                ("ctera_c200-v1", vec!["Ctera C200 V1", "Ctera C200 V1b"]),
                ("generic", vec!["Generic x86/64"]),
                ("glinet_gl-mt6000", vec!["GL.iNet GL-MT6000"]),
                ("untitled", vec!["untitled"]),
            ]
        );
    }

    #[test]
    fn a_profiles_packages_are_the_defaults_with_its_own() {
        let body = br#"{"default_packages":["dnsmasq","wpad-basic-mbedtls","ppp"],
            "device_packages":["kmod-mt7915e","-ppp"],"images":[]}"#;
        let info = profile_info(body).unwrap();
        assert_eq!(
            info.packages.iter().map(String::as_str).collect::<Vec<_>>(),
            ["dnsmasq", "kmod-mt7915e", "wpad-basic-mbedtls"]
        );
    }

    #[test]
    fn targets_and_profiles_that_fit_a_path() {
        assert!(valid_target("mediatek/filogic"));
        assert!(valid_target("x86/64"));
        assert!(!valid_target("x86"));
        assert!(!valid_target("x86/../64"));
        assert!(!valid_target("/64"));
        assert!(valid_profile("glinet_gl-mt6000"));
        assert!(valid_profile("tplink_archer-c7-v5.1"));
        assert!(!valid_profile(".."));
        assert!(!valid_profile("a/b"));
        assert!(!valid_profile(""));
    }
}
