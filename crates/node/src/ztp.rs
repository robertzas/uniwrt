//! The first-boot script of zero-touch images (features/adoption.md, Path B; D-022, D-102):
//! `node-agent/ztp/first-boot.sh` with the controller's values written in. It goes into an
//! image's `/etc/uci-defaults` (the OpenWrt Firmware Selector's "script to run on first boot",
//! or an ASU build), with `node-agent/ztp/uniwrt-hello`, the call-home loop it installs. It
//! holds no secrets: the controller's public key and its addresses. With a one-liner's token
//! (Path D), it runs on a live device: the hello carries the token, and the changes take
//! effect once the script has ended. [`pending`] checks the hello the script sends.

use std::net::IpAddr;

use uniwrt_core::Timestamp;
use uniwrt_model::{
    node::NodeRole,
    ztp::{PendingDevice, PendingSource, ZtpHello},
};

use crate::HostKey;

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
    /// `https://<host>:<port>`, an IPv6 host in brackets.
    pub fn url(&self) -> String {
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
    #[error("a one-liner's token holds only letters, digits, `-` and `_`")]
    BadToken,
}

/// The script for an image of `role`, trusting `controller_key` (an `authorized_keys` line)
/// and calling home to `endpoints` in order; for the one-liner with its `token`.
pub fn first_boot(
    controller_key: &str,
    endpoints: &[Endpoint],
    role: NodeRole,
    token: Option<&str>,
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
    let token = token.unwrap_or_default();
    let token_ok = token.len() <= 64
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !token_ok {
        return Err(ScriptError::BadToken);
    }
    let urls: Vec<String> = endpoints.iter().map(Endpoint::url).collect();
    let values = format!(
        "CONTROLLER_KEY='{key}'\nURLS='{}'\nROLE='{}'\nTOKEN='{token}'\n",
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

/// Where the one-liner leaves the script before checking it.
const ONE_LINER_FILE: &str = "/var/run/uniwrt.sh";

/// The one-liner (Path D) that fetches `script` from `endpoint` with `token`: over HTTPS that
/// OpenWrt can't check (D-102), so it runs the script only when its SHA-256 matches the one
/// written in the command (D-103). Returns the command and the script's hash (hex).
pub fn one_liner(endpoint: &Endpoint, token: &str, script: &str) -> (String, String) {
    let sha256 = data_encoding::HEXLOWER
        .encode(ring::digest::digest(&ring::digest::SHA256, script.as_bytes()).as_ref());
    // IPv4 for names, as the hello does (musl fails a lookup whose AAAA query is refused).
    let family = if endpoint.host.contains(':') {
        "-6"
    } else {
        "-4"
    };
    let url = endpoint.url();
    let command = format!(
        "uclient-fetch {family} --no-check-certificate -qO {ONE_LINER_FILE} '{url}/b/{token}' \
         && echo '{sha256}  {ONE_LINER_FILE}' | sha256sum -cs && sh {ONE_LINER_FILE}"
    );
    (command, sha256)
}

fn role_name(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Gateway => "gateway",
        NodeRole::Ap => "ap",
        NodeRole::Bridge => "bridge",
    }
}

/// The longest text field of a hello.
const MAX_HELLO_TEXT: usize = 128;

/// A ZTP device's hello (`POST /api/v1/ztp/hello`, features/adoption.md, Path B; D-102) as the
/// inbox keeps it, from `address` at `now`, or why it is refused: a unicast MAC, an OpenSSH
/// host key, and fields of at most 128 printable characters, trimmed (the board's name not
/// empty).
pub fn pending(hello: &ZtpHello, address: IpAddr, now: Timestamp) -> Result<PendingDevice, String> {
    if hello.mac.is_multicast() || hello.mac.octets() == [0; 6] {
        return Err(format!("{} is not a device's MAC address", hello.mac));
    }
    let host_key = HostKey::parse(hello.host_key.trim())
        .map_err(|_| "The host key must be an OpenSSH public key".to_owned())?;
    Ok(PendingDevice {
        mac: hello.mac,
        board_name: hello_text("board_name", &hello.board_name, false)?,
        model: hello_text("model", &hello.model, true)?,
        hostname: hello_text("hostname", &hello.hostname, true)?,
        release: hello_text("release", &hello.release, true)?,
        role: hello.role,
        address,
        source: PendingSource::Hello,
        locked: false,
        host_key: host_key.to_openssh().to_owned(),
        fingerprint: host_key.fingerprint().to_owned(),
        first_seen: now,
        last_seen: now,
        job: None,
    })
}

/// A short printable field, trimmed; empty only where `may_be_empty`.
fn hello_text(field: &str, value: &str, may_be_empty: bool) -> Result<String, String> {
    let value = value.trim();
    let fits = value.chars().count() <= MAX_HELLO_TEXT && !value.chars().any(char::is_control);
    if !fits || (value.is_empty() && !may_be_empty) {
        return Err(format!(
            "{field} must be up to {MAX_HELLO_TEXT} printable characters"
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use uniwrt_model::node::NodeRole;

    use super::{Endpoint, ScriptError, first_boot, one_liner};

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
            None,
        )
        .unwrap();
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(&format!("\nCONTROLLER_KEY='{KEY}'\n")));
        assert!(script.contains("\nURLS='https://uniwrt.lan:8443 https://[fd00::5]:443'\n"));
        assert!(script.contains("\nROLE='ap'\n"));
        assert!(script.contains("\nTOKEN=''\n"));
        assert_eq!(script.matches("CONTROLLER_KEY='").count(), 1);
        assert!(!script.contains("ExampleOnly"));
        assert!(script.ends_with("exit 0\n"));
        // The hello goes in whole, before its heredoc's end.
        assert!(script.contains(&format!("<<'HELLO'\n{}HELLO\n", super::HELLO)));
        assert!(!super::HELLO.lines().any(|line| line == "HELLO"));
    }

    #[test]
    fn nothing_a_shell_reads_gets_in() {
        let bad =
            |host: &str| first_boot(KEY, &[endpoint(host, 443)], NodeRole::Ap, None).unwrap_err();
        for host in ["a'b", "$(reboot)", "a b", "-x", "", "a;b", "a/b"] {
            assert_eq!(bad(host), ScriptError::BadHost(host.to_owned()), "{host}");
        }
        assert_eq!(
            first_boot(KEY, &[], NodeRole::Ap, None).unwrap_err(),
            ScriptError::NoEndpoint
        );
        for key in [
            "",
            "ssh-ed25519",
            "ssh-ed25519 AAAA' x",
            "ssh-ed25519 AAAA\nx",
        ] {
            assert_eq!(
                first_boot(key, &[endpoint("uniwrt.lan", 443)], NodeRole::Ap, None).unwrap_err(),
                ScriptError::BadKey,
                "{key:?}"
            );
        }
    }

    #[test]
    fn a_one_liner_carries_its_token() {
        let lan = [endpoint("uniwrt.lan", 443)];
        let script = first_boot(KEY, &lan, NodeRole::Ap, Some("Xk3_a-9")).unwrap();
        assert!(script.contains("\nTOKEN='Xk3_a-9'\n"));
        for token in ["a'b", "$(x)", "a b", &"a".repeat(65)] {
            assert_eq!(
                first_boot(KEY, &lan, NodeRole::Ap, Some(token)).unwrap_err(),
                ScriptError::BadToken,
                "{token}"
            );
        }
    }

    #[test]
    fn the_one_liner_checks_what_it_fetched() {
        let (command, sha256) = one_liner(&endpoint("10.0.0.5", 8443), "Xk3", "echo hi\n");
        assert_eq!(
            sha256,
            "ab08508fdf5ca4da5c4995987bc41c56c048aaa5eeb046417ae4049b7d40286e"
        );
        assert_eq!(
            command,
            format!(
                "uclient-fetch -4 --no-check-certificate -qO /var/run/uniwrt.sh \
                 'https://10.0.0.5:8443/b/Xk3' && echo '{sha256}  /var/run/uniwrt.sh' \
                 | sha256sum -cs && sh /var/run/uniwrt.sh"
            )
        );
        let (v6, _) = one_liner(&endpoint("fd00::5", 443), "Xk3", "");
        assert!(v6.starts_with("uclient-fetch -6 "));
        assert!(v6.contains("'https://[fd00::5]:443/b/Xk3'"));
    }
}
