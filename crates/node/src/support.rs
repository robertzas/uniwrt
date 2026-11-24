//! A node's part of the support bundle (features/observability.md, Troubleshooting assistants;
//! D-144): one exec of a fixed script ([`snapshot`]) prints each command's output under a
//! `=== <name>` line, read back as named sections ([`sections`]), with the options whose
//! names say they hold a secret masked wherever a section is JSON ([`mask_json`], as the UCI
//! explorer masks them).

use std::time::Duration;

use serde_json::Value;
use uniwrt_model::explorer::is_masked_option;

use crate::ExecRequest;

/// What stands for a masked value.
pub const MASK: &str = "********";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: usize = 8 << 20;

/// The commands, each under its section's name; a constant, so nothing is interpolated. The
/// sections that are JSON end in `.json`.
pub const SCRIPT: &str = r#"s() { printf '\n=== %s\n' "$1"; shift; "$@" 2>&1; }
s board.json ubus call system board
s system.json ubus call system info
s wireless.json ubus call network.wireless status
s interfaces.json ubus call network.interface dump
s addresses.txt ip addr
s routes.txt ip route
s routes6.txt ip -6 route
s neighbors.txt ip neigh
s bridges.txt bridge fdb show
s disk.txt df -h
s processes.txt top -b -n 1
s dmesg.txt dmesg
s logread.txt logread
exit 0"#;

/// The snapshot's exec request.
pub fn snapshot() -> ExecRequest {
    ExecRequest {
        argv: ["sh", "-c", SCRIPT].map(str::to_owned).to_vec(),
        stdin: None,
        timeout: TIMEOUT,
        max_output: MAX_OUTPUT,
    }
}

/// The sections' names, in the script's order.
fn names() -> impl Iterator<Item = &'static str> {
    SCRIPT
        .lines()
        .filter_map(|line| line.strip_prefix("s ")?.split_whitespace().next())
}

/// The sections the snapshot printed, by name, in order; JSON sections masked and pretty. A
/// header is the next section's name only, so a log line that looks like one stays in its text.
pub fn sections(stdout: &str) -> Vec<(String, String)> {
    let mut expected = names().peekable();
    let mut sections: Vec<(String, String)> = Vec::new();
    for line in stdout.split_inclusive('\n') {
        if let Some(&next) = expected.peek()
            && line.strip_prefix("=== ").map(str::trim) == Some(next)
        {
            sections.push((next.to_owned(), String::new()));
            expected.next();
            continue;
        }
        if let Some((_, text)) = sections.last_mut() {
            text.push_str(line);
        }
    }
    for (name, text) in &mut sections {
        // The blank line the next section's header starts with.
        if text.ends_with('\n') && text.len() > 1 && text[..text.len() - 1].ends_with('\n') {
            text.pop();
        }
        if std::path::Path::new(name.as_str())
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            && let Ok(mut value) = serde_json::from_str::<Value>(text)
        {
            mask_json(&mut value);
            *text = serde_json::to_string_pretty(&value).unwrap_or_default() + "\n";
        }
    }
    sections
}

/// Masks, at any depth, the values of keys whose names say they hold a secret.
pub fn mask_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_masked_option(key) && !value.is_null() {
                    *value = Value::String(MASK.to_owned());
                } else {
                    mask_json(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask_json),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MASK, mask_json, sections, snapshot};

    #[test]
    fn sections_are_named_and_json_masked() {
        let stdout = "
=== board.json
{\"model\": \"x\"}

=== system.json
{}

=== wireless.json
{\"radio0\": {\"interfaces\": [{\"config\": {\"ssid\": \"home\", \"key\": \"hunter22\", \"sae_password\": \"p\"}}]}}

=== interfaces.json
not JSON
=== logread.txt
=== board.json
line two
";
        let sections = sections(stdout);
        let names: Vec<_> = sections.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "board.json",
                "system.json",
                "wireless.json",
                "interfaces.json"
            ]
        );
        let wireless = &sections[2].1;
        assert!(!wireless.contains("hunter22"), "{wireless}");
        assert!(wireless.contains(MASK));
        assert!(wireless.contains("home"));
        // A name out of turn is text.
        assert_eq!(
            sections[3].1,
            "not JSON\n=== logread.txt\n=== board.json\nline two\n"
        );
    }

    #[test]
    fn masking_reaches_every_depth_and_leaves_other_values() {
        let mut value = json!({ "a": [{ "psk": "x", "n": 1 }], "token": null, "password": "y" });
        mask_json(&mut value);
        assert_eq!(
            value,
            json!({ "a": [{ "psk": MASK, "n": 1 }], "token": null, "password": MASK })
        );
        assert_eq!(snapshot().argv[..2], ["sh", "-c"]);
    }
}
