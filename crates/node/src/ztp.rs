//! The first-boot script of zero-touch images (features/adoption.md, Path B; D-022, D-102):
//! `node-agent/ztp/first-boot.sh` with the controller's values written in. It goes into an
//! image's `/etc/uci-defaults` (the OpenWrt Firmware Selector's "script to run on first boot",
//! or an ASU build), with `node-agent/ztp/uniwrt-hello`, the call-home loop it installs. It
//! holds no secrets: the controller's public key and its addresses.

use uniwrt_model::node::NodeRole;

const TEMPLATE: &str = include_str!("../../../node-agent/ztp/first-boot.sh");
/// The call-home loop the script installs as `/usr/sbin/uniwrt-hello`.
const HELLO: &str = include_str!("../../../node-agent/ztp/uniwrt-hello");
const HELLO_START: &str = "cat >/usr/sbin/uniwrt-hello <<'HELLO'\n";
const VALUES_START: &str = "# --- values (the controller writes these) ---\n";
const VALUES_END: &str = "# --- end of values ---\n";

/// Where a ZTP device finds the controller, tried in order: `https://<host>:<port>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// A DNS name or an IP address (IPv6 without brackets).
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    fn url(&self) -> String {
        if self.host.contains(':') {
            format!("https://[{}]:{}", self.host, self.port)
        } else {
            format!("https://{}:{}", self.host, self.port)
        }
    }

    /// Letters, digits, `.`, `-` and `:` only: nothing a shell or a URL reads otherwise.
    fn valid(&self) -> bool {
        !self.host.is_empty()
            && self.host.len() <= 253
            && !self.host.starts_with(['-', '.'])
            && self
                .host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
            && self.port != 0
    }
}

/// Why a script can't be written.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ScriptError {
    #[error("no address to call home to")]
    NoEndpoint,
    #[error("{0} is not a host name or an IP address")]
    BadHost(String),
    #[error("the controller's key is not one OpenSSH line")]
    BadKey,
}

/// The script for an image of `role`, trusting `controller_key` (an `authorized_keys` line)
/// and calling home to `endpoints` in order.
pub fn first_boot(
    controller_key: &str,
    endpoints: &[Endpoint],
    role: NodeRole,
) -> Result<String, ScriptError> {
    if endpoints.is_empty() {
        return Err(ScriptError::NoEndpoint);
    }
    if let Some(bad) = endpoints.iter().find(|endpoint| !endpoint.valid()) {
        return Err(ScriptError::BadHost(bad.host.clone()));
    }
    let key = controller_key.trim();
    let key_ok = !key.is_empty()
        && key.split(' ').count() >= 2
        && key
            .chars()
            .all(|c| c.is_ascii_graphic() && c != '\'' || c == ' ');
    if !key_ok {
        return Err(ScriptError::BadKey);
    }
    let urls: Vec<String> = endpoints.iter().map(Endpoint::url).collect();
    let values = format!(
        "CONTROLLER_KEY='{key}'\nURLS='{}'\nROLE='{}'\n",
        urls.join(" "),
        role_name(role)
    );
    let (head, rest) = TEMPLATE
        .split_once(VALUES_START)
        .expect("the template marks its values");
    let (_, tail) = rest
        .split_once(VALUES_END)
        .expect("the template ends its values");
    let (before, after) = tail
        .split_once(HELLO_START)
        .expect("the template writes the hello");
    Ok(format!(
        "{head}{VALUES_START}{values}{VALUES_END}{before}{HELLO_START}{HELLO}{after}"
    ))
}

fn role_name(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Gateway => "gateway",
        NodeRole::Ap => "ap",
        NodeRole::Bridge => "bridge",
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_model::node::NodeRole;

    use super::{Endpoint, ScriptError, first_boot};

    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK0wmN/Cr3JXqmLW7u+g9pTh+wyqDHpSQEIQczXkVx9q uniwrt";

    fn endpoint(host: &str, port: u16) -> Endpoint {
        Endpoint {
            host: host.to_owned(),
            port,
        }
    }

    #[test]
    fn the_values_are_written_in_once() {
        let script = first_boot(
            KEY,
            &[endpoint("uniwrt.lan", 8443), endpoint("fd00::5", 443)],
            NodeRole::Ap,
        )
        .unwrap();
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(&format!("\nCONTROLLER_KEY='{KEY}'\n")));
        assert!(script.contains("\nURLS='https://uniwrt.lan:8443 https://[fd00::5]:443'\n"));
        assert!(script.contains("\nROLE='ap'\n"));
        assert_eq!(script.matches("CONTROLLER_KEY='").count(), 1);
        assert!(!script.contains("ExampleOnly"));
        assert!(script.ends_with("exit 0\n"));
        // The hello goes in whole, before its heredoc's end.
        assert!(script.contains(&format!("<<'HELLO'\n{}HELLO\n", super::HELLO)));
        assert!(!super::HELLO.lines().any(|line| line == "HELLO"));
    }

    #[test]
    fn nothing_a_shell_reads_gets_in() {
        let bad = |host: &str| first_boot(KEY, &[endpoint(host, 443)], NodeRole::Ap).unwrap_err();
        for host in ["a'b", "$(reboot)", "a b", "-x", "", "a;b", "a/b"] {
            assert_eq!(bad(host), ScriptError::BadHost(host.to_owned()), "{host}");
        }
        assert_eq!(
            first_boot(KEY, &[], NodeRole::Ap).unwrap_err(),
            ScriptError::NoEndpoint
        );
        for key in [
            "",
            "ssh-ed25519",
            "ssh-ed25519 AAAA' x",
            "ssh-ed25519 AAAA\nx",
        ] {
            assert_eq!(
                first_boot(key, &[endpoint("uniwrt.lan", 443)], NodeRole::Ap).unwrap_err(),
                ScriptError::BadKey,
                "{key:?}"
            );
        }
    }
}
