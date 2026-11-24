//! The feeds of a release a node is offered (D-147, M9.0c1b2), derived from the ones it runs:
//! OpenWrt's feeds name the release in their path (`releases/24.10.8/packages/x86_64/base`),
//! so the next release's are the same with that segment replaced, the package architecture's
//! too where it changes, and the kernel modules' directory (`kmods/6.6.144-1-50da…`, which
//! names the kernel build) replaced by the one the release's `profiles.json` names. A feed
//! whose path doesn't name the release (a feed of the user's own) is left out, as is a kernel
//! modules feed when the next release's directory isn't known. The host is kept, so a node
//! fetching from a mirror of OpenWrt's gets that mirror's next release. Whether the feeds are
//! opkg's or apk's is the feed set's: a feed is a base path either way.

use uniwrt_model::feeds::NextRelease;

use crate::Feed;

/// `current`'s feeds on `next`, for a node running `version` on `arch`.
pub fn derive(current: &[Feed], version: &str, arch: &str, next: &NextRelease) -> Vec<Feed> {
    let mut derived: Vec<Feed> = Vec::new();
    for feed in current {
        let segments: Vec<&str> = feed.path().split('/').collect();
        if !segments.contains(&version) {
            continue;
        }
        let mut path = Vec::with_capacity(segments.len());
        let mut kmods = false;
        let mut known = true;
        for segment in segments {
            let replaced = if kmods {
                let Some(dir) = next.kmods.as_deref() else {
                    known = false;
                    break;
                };
                dir
            } else if segment == version {
                next.release.as_str()
            } else if segment == arch {
                next.arch.as_str()
            } else {
                segment
            };
            kmods = segment == "kmods";
            path.push(replaced);
        }
        if !known {
            continue;
        }
        if let Ok(feed) = feed.with_path(&path.join("/"))
            && !derived.contains(&feed)
        {
            derived.push(feed);
        }
    }
    derived
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 24.10.8's x86/64 feeds, as its image lists them (`/etc/opkg/distfeeds.conf`).
    const OPKG: [&str; 7] = [
        "https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/packages",
        "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base",
        "https://downloads.openwrt.org/releases/24.10.8/targets/x86/64/kmods/6.6.144-1-50daf8372d971124fb3519e8d87e02ae",
        "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/luci",
        "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/packages",
        "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/routing",
        "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/telephony",
    ];

    /// 25.12.6's (`/etc/apk/repositories.d/distfeeds.list`); it adds a `video` feed.
    const APK: [&str; 8] = [
        "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/packages/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/base/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/kmods/6.12.112-1-b4125196ecec75391093e1e2edeba490/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/luci/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/packages/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/routing/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/telephony/packages.adb",
        "https://downloads.openwrt.org/releases/25.12.6/packages/x86_64/video/packages.adb",
    ];

    fn next(release: &str, kmods: Option<&str>, arch: &str) -> NextRelease {
        NextRelease {
            release: release.to_owned(),
            target: "x86/64".to_owned(),
            arch: arch.to_owned(),
            manager: None,
            kmods: kmods.map(str::to_owned),
            packages: Vec::new(),
            images: Vec::new(),
        }
    }

    fn feeds(urls: &[&str]) -> Vec<Feed> {
        urls.iter().map(|url| Feed::parse(url).unwrap()).collect()
    }

    #[test]
    fn the_next_branchs_feeds_are_the_ones_its_image_lists() {
        let to = next(
            "25.12.6",
            Some("6.12.112-1-b4125196ecec75391093e1e2edeba490"),
            "x86_64",
        );
        let derived = derive(&feeds(&OPKG), "24.10.8", "x86_64", &to);
        assert_eq!(derived, feeds(&APK[..7]), "all but 25.12's new video feed");
    }

    #[test]
    fn a_feed_not_of_the_release_and_an_unknown_kmods_directory_are_left_out() {
        let mut current = feeds(&OPKG);
        current.push(Feed::parse("https://example.com/my/feed").unwrap());
        current.push(
            Feed::parse("http://mirror.lan/openwrt/releases/24.10.8/packages/x86_64/base").unwrap(),
        );
        let derived = derive(
            &current,
            "24.10.8",
            "x86_64",
            &next("24.10.9", None, "x86_64"),
        );
        let urls: Vec<String> = derived.iter().map(Feed::url).collect();
        assert_eq!(urls.len(), 7, "{urls:#?}");
        assert!(urls.iter().all(|url| url.contains("/24.10.9/")));
        assert!(
            !urls
                .iter()
                .any(|url| url.contains("kmods") || url.contains("example.com"))
        );
        assert!(urls.contains(
            &"http://mirror.lan/openwrt/releases/24.10.9/packages/x86_64/base".to_owned()
        ));
    }

    #[test]
    fn a_new_package_architecture_moves_the_feeds_with_it() {
        let current =
            feeds(&["https://downloads.openwrt.org/releases/24.10.8/packages/arm_cortex-a7/base"]);
        let derived = derive(
            &current,
            "24.10.8",
            "arm_cortex-a7",
            &next("25.12.6", None, "arm_cortex-a7_neon-vfpv4"),
        );
        assert_eq!(
            derived[0].url(),
            "https://downloads.openwrt.org/releases/25.12.6/packages/arm_cortex-a7_neon-vfpv4/base"
        );
    }
}
