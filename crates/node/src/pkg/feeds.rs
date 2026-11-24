//! The feeds a node's package manager reads (D-147), kept with its inventory so the controller
//! mirrors them: opkg reads `/etc/opkg.conf`, then `/etc/opkg/*.conf` in order, and the first
//! `src` line of a name wins; apk reads `/etc/apk/repositories`, then
//! `/etc/apk/repositories.d/*.list`. One fixed script prints them; a file that is missing is
//! skipped, so it always succeeds.

use std::collections::BTreeSet;

use uniwrt_model::{facts::PackageManager, inventory::NodeFeed};

/// The script for opkg.
pub const OPKG: &str = "cat /etc/opkg.conf /etc/opkg/*.conf 2>/dev/null; true";
/// The script for apk.
pub const APK: &str = "cat /etc/apk/repositories /etc/apk/repositories.d/*.list 2>/dev/null; true";

/// The command whose output [`parse`] reads.
pub fn command(manager: PackageManager) -> [&'static str; 3] {
    let script = match manager {
        PackageManager::Opkg => OPKG,
        PackageManager::Apk => APK,
    };
    ["sh", "-c", script]
}

/// The feeds in the output of [`command`], in the manager's order, each once.
pub fn parse(manager: PackageManager, output: &str) -> Vec<NodeFeed> {
    let mut feeds: Vec<NodeFeed> = Vec::new();
    let mut names = BTreeSet::new();
    for line in output.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first().is_none_or(|word| word.starts_with('#')) {
            continue;
        }
        let feed = match manager {
            PackageManager::Opkg => match words.as_slice() {
                ["src" | "src/gz", name, url, ..] => {
                    if !names.insert((*name).to_owned()) {
                        continue;
                    }
                    NodeFeed {
                        name: Some((*name).to_owned()),
                        url: (*url).to_owned(),
                    }
                }
                _ => continue,
            },
            PackageManager::Apk => NodeFeed {
                name: words
                    .first()
                    .and_then(|word| word.strip_prefix('@'))
                    .filter(|_| words.len() > 1)
                    .map(str::to_owned),
                url: (*words.last().unwrap_or(&"")).to_owned(),
            },
        };
        if !feeds.iter().any(|kept| kept.url == feed.url) {
            feeds.push(feed);
        }
    }
    feeds
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{facts::PackageManager, inventory::NodeFeed};

    use super::{command, parse};

    fn feed(name: Option<&str>, url: &str) -> NodeFeed {
        NodeFeed {
            name: name.map(str::to_owned),
            url: url.to_owned(),
        }
    }

    #[test]
    fn opkg_feeds_keep_the_first_line_of_each_name() {
        assert_eq!(command(PackageManager::Opkg)[..2], ["sh", "-c"]);
        let conf = "\
dest root /
option overlay_root /overlay
src/gz openwrt_core https://downloads.openwrt.org/releases/24.10.4/targets/x86/64/packages
src/gz openwrt_base https://downloads.openwrt.org/releases/24.10.4/packages/x86_64/base
# src/gz openwrt_old https://example.org/old
src/gz openwrt_core https://mirror.example.org/core
src local file:///root/packages
";
        assert_eq!(
            parse(PackageManager::Opkg, conf),
            [
                feed(
                    Some("openwrt_core"),
                    "https://downloads.openwrt.org/releases/24.10.4/targets/x86/64/packages"
                ),
                feed(
                    Some("openwrt_base"),
                    "https://downloads.openwrt.org/releases/24.10.4/packages/x86_64/base"
                ),
                feed(Some("local"), "file:///root/packages"),
            ]
        );
    }

    #[test]
    fn apk_feeds_are_urls_with_their_tag() {
        let lists = "\
https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/packages/packages.adb

#https://example.org/old/packages.adb
@extra https://example.org/extra/packages.adb
https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/packages/packages.adb
";
        assert_eq!(
            parse(PackageManager::Apk, lists),
            [
                feed(
                    None,
                    "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/packages/packages.adb"
                ),
                feed(Some("extra"), "https://example.org/extra/packages.adb"),
            ]
        );
    }
}
