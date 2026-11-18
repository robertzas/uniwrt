//! Reading a list: each line's blocked domains, whatever the list's format. Hosts files
//! (`0.0.0.0 a.com b.com`), plain domains (`a.com`, `*.a.com`), adblock rules (`||a.com^`,
//! `||a.com^$important`) and dnsmasq lines (`local=/a.com/`, `server=/a.com/`, `address=/a.com/`
//! to nowhere) are read; comments, exceptions (`@@||…`), rules with other options, forwards
//! (`server=/a.com/1.2.3.4`) and redirects are skipped, as are invalid names, addresses and
//! single labels (a list never blocks a whole top-level domain).

use std::net::IpAddr;

/// The blocked domains of one `line`, normalized ([`domain`]), into `out`.
pub fn line(line: &[u8], out: &mut Vec<String>) {
    let Ok(line) = std::str::from_utf8(line) else {
        return;
    };
    let line = line.trim();
    if line.is_empty() || line.starts_with(['#', '!', '[']) {
        return;
    }
    if let Some(rule) = line.strip_prefix("||") {
        if let Some((name, rest)) = rule.split_once('^')
            && (rest.is_empty() || rest == "$important")
        {
            push(name, out);
        }
        return;
    }
    if let Some(rest) = ["local=", "server=", "address="]
        .iter()
        .find_map(|key| line.strip_prefix(key).map(|rest| (*key, rest)))
    {
        dnsmasq(rest.0, rest.1, out);
        return;
    }
    let text = line.split_once('#').map_or(line, |(text, _)| text);
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return;
    };
    if first.parse::<IpAddr>().is_ok() {
        for name in words {
            push(name, out);
        }
    } else if words.next().is_none() {
        push(first, out);
    }
}

/// A dnsmasq `key=/d1/d2/target` line: blocks for `local=`, for `server=` without a target and
/// for `address=` to nowhere.
fn dnsmasq(key: &str, rest: &str, out: &mut Vec<String>) {
    let Some(rest) = rest.strip_prefix('/') else {
        return;
    };
    let Some((names, target)) = rest.rsplit_once('/') else {
        return;
    };
    let blocks = match key {
        "local=" | "server=" => target.is_empty(),
        _ => matches!(target, "" | "#" | "0.0.0.0" | "::" | "127.0.0.1" | "::1"),
    };
    if blocks {
        for name in names.split('/') {
            push(name, out);
        }
    }
}

fn push(name: &str, out: &mut Vec<String>) {
    if let Some(name) = domain(name).filter(|name| name.contains('.')) {
        out.push(name);
    }
}

/// `name` lowercased, without a leading `*.` or `.` or a trailing `.`; `None` unless it is a
/// host name: labels of 1–63 letters, digits, `-` or `_`, at most 253 bytes, not all digits (an
/// address) and none of the names hosts files give the loopback.
pub fn domain(name: &str) -> Option<String> {
    let name = name.trim();
    let name = name
        .strip_prefix("*.")
        .or_else(|| name.strip_prefix('.'))
        .unwrap_or(name);
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.is_empty() || name.len() > 253 {
        return None;
    }
    let labels_ok = name.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    });
    let numeric = name.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    let name = name.to_ascii_lowercase();
    let loopback = matches!(
        name.as_str(),
        "localhost"
            | "localhost.localdomain"
            | "local"
            | "broadcasthost"
            | "ip6-localhost"
            | "ip6-loopback"
            | "0.0.0.0"
    );
    (labels_ok && !numeric && !loopback).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{domain, line};

    fn read(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for l in text.lines() {
            line(l.as_bytes(), &mut out);
        }
        out
    }

    #[test]
    fn every_format_yields_its_domains() {
        let text = "# a comment\n! adblock comment\n[Adblock Plus 2.0]\n\n\
            0.0.0.0 Ads.Example.com tracker.example.net # inline\n\
            127.0.0.1 localhost\n::1 ip6-localhost\n\
            plain.example.org\n*.wild.example.org\n\
            ||adblock.example.com^\n||important.example.com^$important\n\
            ||third.example.com^$third-party\n@@||allowed.example.com^\n\
            local=/oisd.example.com/\nserver=/one.example.com/two.example.com/\n\
            server=/forward.example.com/1.1.1.1\nserver=/reallow.example.com/#\n\
            address=/nowhere.example.com/0.0.0.0\naddress=/hash.example.com/#\n\
            address=/redirect.example.com/192.0.2.1\n";
        assert_eq!(
            read(text),
            [
                "ads.example.com",
                "tracker.example.net",
                "plain.example.org",
                "wild.example.org",
                "adblock.example.com",
                "important.example.com",
                "oisd.example.com",
                "one.example.com",
                "two.example.com",
                "nowhere.example.com",
                "hash.example.com",
            ]
        );
    }

    #[test]
    fn junk_and_single_labels_are_skipped() {
        let text = "com\n0.0.0.0 0.0.0.0\n0.0.0.0 local\n1.2.3.4\nbad name.com\n\
            under_score.example.com\nexample..com\ntrailing.example.com.\n";
        assert_eq!(
            read(text),
            ["under_score.example.com", "trailing.example.com"]
        );
        let mut out = Vec::new();
        line(b"\xff\xfe.example.com", &mut out);
        assert_eq!(out, Vec::<String>::new());
        assert_eq!(domain(&format!("{}.com", "a".repeat(64))), None);
        assert_eq!(domain("Example.COM"), Some("example.com".to_owned()));
    }
}
