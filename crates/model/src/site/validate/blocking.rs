//! DNS filtering (D-133): the catalog's lists, custom URLs, the allowed and blocked domains and
//! the bypassed clients.

use std::collections::BTreeSet;

use super::{Issues, names::is_dns_name};
use crate::site::{SiteConfig, blocking::catalog_list};

/// The most custom lists.
pub const MAX_CUSTOM: usize = 16;
/// The most allowed, and blocked, domains.
pub const MAX_DOMAINS: usize = 1_000;
/// The most bypassed clients.
pub const MAX_BYPASS: usize = 256;

pub(super) fn blocking(config: &SiteConfig, issues: &mut Issues) {
    let at = "services/blocking";
    let blocking = &config.services.blocking;
    let mut ids = BTreeSet::new();
    for (i, id) in blocking.lists.iter().enumerate() {
        if catalog_list(id).is_none() {
            issues.error(format!("{at}/lists/{i}"), format!("No list is called {id}"));
        } else if !ids.insert(id) {
            issues.error(format!("{at}/lists/{i}"), "This list is named twice");
        }
    }
    if blocking.custom.len() > MAX_CUSTOM {
        issues.error(
            format!("{at}/custom"),
            format!("At most {MAX_CUSTOM} custom lists"),
        );
    }
    let mut urls = BTreeSet::new();
    for (i, list) in blocking.custom.iter().enumerate() {
        let at = format!("{at}/custom/{i}");
        let name = list.name.trim();
        if name.is_empty()
            || list.name.chars().count() > 64
            || list.name.chars().any(char::is_control)
        {
            issues.error(format!("{at}/name"), "Name the list in up to 64 characters");
        }
        if !is_list_url(&list.url) {
            issues.error(
                format!("{at}/url"),
                "A list is an http:// or https:// URL, such as https://example.com/hosts.txt",
            );
        } else if !urls.insert(list.url.as_str()) {
            issues.error(format!("{at}/url"), "This list is named twice");
        }
    }
    let allowed = domains(&blocking.allow, &format!("{at}/allow"), issues);
    let denied = domains(&blocking.deny, &format!("{at}/deny"), issues);
    for (i, domain) in blocking.deny.iter().enumerate() {
        if allowed.contains(domain.as_str()) {
            issues.error(format!("{at}/deny/{i}"), format!("{domain} is allowed too"));
        }
    }
    if blocking.bypass.len() > MAX_BYPASS {
        issues.error(
            format!("{at}/bypass"),
            format!("At most {MAX_BYPASS} clients bypass the filter"),
        );
    }
    let mut macs = BTreeSet::new();
    for (i, mac) in blocking.bypass.iter().enumerate() {
        if mac.is_multicast() {
            issues.error(format!("{at}/bypass/{i}"), format!("{mac} is not a client"));
        } else if !macs.insert(mac) {
            issues.error(format!("{at}/bypass/{i}"), "This client is named twice");
        }
    }
    if blocking.enabled && ids.is_empty() && blocking.custom.is_empty() && denied.is_empty() {
        issues.warn(
            format!("{at}/lists"),
            "Nothing is blocked: pick a list or block a domain",
        );
    }
}

/// The valid domains of `list`, each a lowercase DNS name named once; the others are errors.
fn domains<'a>(list: &'a [String], at: &str, issues: &mut Issues) -> BTreeSet<&'a str> {
    if list.len() > MAX_DOMAINS {
        issues.error(at, format!("At most {MAX_DOMAINS} domains"));
    }
    let mut seen = BTreeSet::new();
    for (i, domain) in list.iter().enumerate() {
        if !is_dns_name(domain) || domain.bytes().any(|b| b.is_ascii_uppercase()) {
            issues.error(
                format!("{at}/{i}"),
                "A domain is a lowercase DNS name, such as ads.example.com",
            );
        } else if !seen.insert(domain.as_str()) {
            issues.error(format!("{at}/{i}"), "This domain is named twice");
        }
    }
    seen
}

/// An `http://` or `https://` URL with a host, without characters a URL wouldn't carry plainly.
pub fn is_list_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    url.len() <= 512
        && !host.is_empty()
        && url
            .bytes()
            .all(|b| (b'!'..=b'~').contains(&b) && !b"'\"\\`<>".contains(&b))
}
