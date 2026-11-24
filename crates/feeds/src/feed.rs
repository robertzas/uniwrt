//! Feeds as the mirror knows them (D-147): where a feed's index and packages are upstream
//! ([`Feed`], parsed from a node's feed line), and the path the mirror keeps and serves them
//! under: the upstream host, then its path, so `https://downloads.openwrt.org/releases/…/base`
//! is kept in `<root>/downloads.openwrt.org/releases/…/base` and served at
//! `/feeds/downloads.openwrt.org/releases/…/base/<file>`. Every part of such a path is checked
//! ([`valid_segment`]), so none climbs out of the mirror's directory.

use crate::Error;

/// The most segments a feed's path may have.
const MAX_SEGMENTS: usize = 16;
/// The longest a feed's path may be.
const MAX_PATH: usize = 512;
/// The longest one segment (a directory or a file name) may be.
const MAX_SEGMENT: usize = 160;

/// A feed: where its index and packages are upstream.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Feed {
    /// Fetched over HTTPS (else plain HTTP, as the node's line says).
    secure: bool,
    /// The host, lowercase, with its port where the URL names one.
    host: String,
    /// The base path, without a leading or trailing slash.
    path: String,
}

impl Feed {
    /// The feed of a node's feed URL: opkg's base URL, or apk's index URL (`…/packages.adb`).
    pub fn parse(url: &str) -> Result<Self, Error> {
        let invalid = |why: &str| Error::Invalid(format!("feed {url}: {why}"));
        let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(invalid("not an http(s) URL"));
        };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = host.to_ascii_lowercase();
        if !valid_host(&host) {
            return Err(invalid("no host the mirror can keep"));
        }
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix("/packages.adb").unwrap_or(path);
        let segments = path.split('/').count();
        if path.is_empty()
            || path.len() > MAX_PATH
            || segments > MAX_SEGMENTS
            || !path.split('/').all(valid_segment)
        {
            return Err(invalid("a path the mirror can't keep"));
        }
        Ok(Self {
            secure,
            host,
            path: path.to_owned(),
        })
    }

    /// Its base URL upstream.
    pub fn url(&self) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        format!("{scheme}://{}/{}", self.host, self.path)
    }

    /// The URL of one of its files upstream.
    pub fn file_url(&self, file: &str) -> String {
        format!("{}/{file}", self.url())
    }

    /// Where the mirror keeps and serves it: the host, then the path.
    pub fn mirror_path(&self) -> String {
        format!("{}/{}", self.host, self.path)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Its base path, without a leading or trailing slash.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The same host and scheme with another base path, checked as a node's line is.
    pub fn with_path(&self, path: &str) -> Result<Self, Error> {
        let scheme = if self.secure { "https" } else { "http" };
        Self::parse(&format!("{scheme}://{}/{path}", self.host))
    }
}

/// Whether `host` (lowercase) is a DNS name or IPv4 address, with an optional port.
fn valid_host(host: &str) -> bool {
    let (name, port) = host.split_once(':').unwrap_or((host, ""));
    let port_ok = port.is_empty() || (port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()));
    port_ok
        && (1..=253).contains(&name.len())
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Whether `segment` is one directory or file name the mirror keeps: no `.` or `..`, no
/// separator, only the characters OpenWrt's paths and package names use.
pub fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= MAX_SEGMENT
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+~-=@".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opkg_and_apk_lines_name_the_same_kind_of_feed() {
        let opkg =
            Feed::parse("https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base")
                .unwrap();
        assert_eq!(
            opkg.mirror_path(),
            "downloads.openwrt.org/releases/24.10.8/packages/x86_64/base"
        );
        assert_eq!(
            opkg.file_url("Packages.gz"),
            "https://downloads.openwrt.org/releases/24.10.8/packages/x86_64/base/Packages.gz"
        );
        let apk = Feed::parse(
            "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/kmods/6.12.112-1-b4125196ecec75391093e1e2edeba490/packages.adb",
        )
        .unwrap();
        assert_eq!(
            apk.url(),
            "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/kmods/6.12.112-1-b4125196ecec75391093e1e2edeba490"
        );
        let custom = Feed::parse("http://Mirror.Example:8080/openwrt/").unwrap();
        assert_eq!(custom.url(), "http://mirror.example:8080/openwrt");
        assert_eq!(custom.host(), "mirror.example:8080");
    }

    #[test]
    fn paths_that_could_leave_the_mirror_are_refused() {
        for url in [
            "ftp://downloads.openwrt.org/x",
            "https://downloads.openwrt.org",
            "https://downloads.openwrt.org/",
            "https://downloads.openwrt.org/releases/../../etc",
            "https://downloads.openwrt.org/releases/./x",
            "https://downloads.openwrt.org/a//b",
            "https://down loads.openwrt.org/x",
            "https://../x",
            "https://-x.org/x",
            "https://host:99999999/x",
            "https://host/x%2F..",
        ] {
            assert!(Feed::parse(url).is_err(), "{url}");
        }
        assert!(valid_segment(
            "libubox20240329_2024.03.29~eb9bcb64-r1_x86_64.ipk"
        ));
        assert!(!valid_segment(".."));
        assert!(!valid_segment("a/b"));
    }
}
