//! What a node holds now, read right before rendering and diffing (features/lifecycle.md,
//! Render → diff → apply): its UCI packages ([`packages`]) and whether its services start at
//! boot ([`services`]). One command each; names reach the script as arguments, never in its
//! text. [`all`] reads every package in `/etc/config`, for the UCI explorer (D-143).

use std::collections::BTreeMap;

use crate::{Error, ExecOutput, ExecRequest, Result};

use super::valid_name;

/// Prints each package as `<name> <bytes>` and its file, or `<name> -` when it has none.
pub const PACKAGES_SCRIPT: &str = r#"for p; do if [ -f "/etc/config/$p" ]; then printf '%s %s\n' "$p" "$(wc -c <"/etc/config/$p")"; cat "/etc/config/$p"; else printf '%s -\n' "$p"; fi; done"#;

/// Prints every file of `/etc/config` whose name is a UCI package name as [`PACKAGES_SCRIPT`]
/// does (`<name> <bytes>`, then the file); others, such as `*.bak`, are skipped, as `uci`
/// can't load them.
pub const ALL_SCRIPT: &str = r#"for f in /etc/config/*; do p=${f##*/}; case $p in ''|*[!A-Za-z0-9_-]*) continue;; esac; [ -f "$f" ] || continue; printf '%s %s\n' "$p" "$(wc -c <"$f")"; cat "$f"; done"#;

/// Prints `<name> on|off|absent` for each service.
pub const SERVICES_SCRIPT: &str = r#"for s; do if [ -x "/etc/init.d/$s" ]; then if "/etc/init.d/$s" enabled; then echo "$s on"; else echo "$s off"; fi; else echo "$s absent"; fi; done"#;

/// Whether a service starts at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    On,
    Off,
    /// Its init script isn't installed.
    Absent,
}

/// A read, and how to understand its output.
#[derive(Debug, Clone)]
pub struct Read<T> {
    pub request: ExecRequest,
    parse: fn(&[u8]) -> Result<T>,
}

impl<T> Read<T> {
    pub fn parse(&self, output: ExecOutput) -> Result<T> {
        let output = output.check(&self.request.argv[2])?;
        (self.parse)(&output.stdout)
    }
}

fn read<T>(
    script: &str,
    names: &[&str],
    what: &str,
    parse: fn(&[u8]) -> Result<T>,
) -> Result<Read<T>> {
    if let Some(bad) = names.iter().find(|name| !valid_name(name)) {
        return Err(Error::Invalid {
            what: format!("{what} name {bad:?}"),
        });
    }
    let mut argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "sh".to_owned(),
    ];
    argv.extend(names.iter().map(|&name| name.to_owned()));
    Ok(Read {
        request: ExecRequest::new(argv),
        parse,
    })
}

/// The files of `names` in `/etc/config`, `None` for each the node lacks. Files are UTF-8,
/// as the uci crate reads them.
pub fn packages(names: &[&str]) -> Result<Read<BTreeMap<String, Option<String>>>> {
    read(PACKAGES_SCRIPT, names, "package", parse_packages)
}

/// Every package in `/etc/config`: its text, or why it isn't text (a file that isn't UTF-8,
/// which the uci crate can't read). A file that changes while it is read may fail the read.
pub fn all() -> Read<BTreeMap<String, std::result::Result<String, String>>> {
    let argv = ["sh", "-c", ALL_SCRIPT, "sh"].map(str::to_owned);
    Read {
        request: ExecRequest::new(argv),
        parse: parse_all,
    }
}

/// Whether each of `names` starts at boot.
pub fn services(names: &[&str]) -> Result<Read<BTreeMap<String, ServiceState>>> {
    read(SERVICES_SCRIPT, names, "service", parse_services)
}

fn protocol(what: impl Into<String>) -> Error {
    Error::Protocol(format!("reading the node's packages: {}", what.into()))
}

fn parse_packages(out: &[u8]) -> Result<BTreeMap<String, Option<String>>> {
    let mut packages = BTreeMap::new();
    for (name, bytes) in entries(out)? {
        let content = match bytes {
            Some(bytes) => Some(
                String::from_utf8(bytes.to_vec())
                    .map_err(|_| protocol(format!("/etc/config/{name} is not UTF-8")))?,
            ),
            None => None,
        };
        packages.insert(name, content);
    }
    Ok(packages)
}

fn parse_all(out: &[u8]) -> Result<BTreeMap<String, std::result::Result<String, String>>> {
    Ok(entries(out)?
        .into_iter()
        .filter_map(|(name, bytes)| {
            let text = String::from_utf8(bytes?.to_vec())
                .map_err(|_| format!("/etc/config/{name} is not UTF-8"));
            Some((name, text))
        })
        .collect())
}

/// A file's name and its bytes, `None` where the node lacks it.
type Entry<'a> = (String, Option<&'a [u8]>);

/// Each `<name> <bytes>` header and the bytes after it, or `None` for `<name> -`.
fn entries(mut out: &[u8]) -> Result<Vec<Entry<'_>>> {
    let mut entries = Vec::new();
    while !out.is_empty() {
        let end = out
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| protocol("a header without its line end"))?;
        let header =
            std::str::from_utf8(&out[..end]).map_err(|_| protocol("a header in binary"))?;
        out = &out[end + 1..];
        let (name, size) = header
            .split_once(' ')
            .ok_or_else(|| protocol(format!("the header {header:?}")))?;
        let content = if size == "-" {
            None
        } else {
            let size: usize = size
                .trim()
                .parse()
                .map_err(|_| protocol(format!("the header {header:?}")))?;
            let bytes = out
                .get(..size)
                .ok_or_else(|| protocol(format!("{name} is shorter than {size} bytes")))?;
            out = &out[size..];
            Some(bytes)
        };
        entries.push((name.to_owned(), content));
    }
    Ok(entries)
}

fn parse_services(out: &[u8]) -> Result<BTreeMap<String, ServiceState>> {
    let out = std::str::from_utf8(out).map_err(|_| protocol("services in binary"))?;
    out.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let state = match line.rsplit_once(' ') {
                Some((name, "on")) => (name, ServiceState::On),
                Some((name, "off")) => (name, ServiceState::Off),
                Some((name, "absent")) => (name, ServiceState::Absent),
                _ => return Err(protocol(format!("the service line {line:?}"))),
            };
            Ok((state.0.to_owned(), state.1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::process::Command as Process;

    use super::*;
    use crate::ExitStatus;

    fn run(request: &ExecRequest, root: &std::path::Path) -> ExecOutput {
        // The scripts read absolute paths: run them with /etc rewritten into a temporary root.
        let script = request.argv[2].replace("/etc/", &format!("{}/etc/", root.display()));
        let out = Process::new("sh")
            .arg("-c")
            .arg(script)
            .args(&request.argv[3..])
            .output()
            .unwrap();
        ExecOutput {
            status: ExitStatus::Code(u32::try_from(out.status.code().unwrap()).unwrap()),
            stdout: out.stdout,
            stderr: out.stderr,
        }
    }

    #[test]
    fn packages_are_read_byte_exact_and_missing_ones_are_none() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("etc/config")).unwrap();
        let network = "config interface 'lan'\n\toption proto 'static'\n\n";
        std::fs::write(root.path().join("etc/config/network"), network).unwrap();
        // No line end at the end, and a header-like line inside.
        std::fs::write(root.path().join("etc/config/system"), "x 12\nsystem -").unwrap();
        let read = packages(&["network", "wireless", "system"]).unwrap();
        let got = read.parse(run(&read.request, root.path())).unwrap();
        assert_eq!(got["network"].as_deref(), Some(network));
        assert_eq!(got["wireless"], None);
        assert_eq!(got["system"].as_deref(), Some("x 12\nsystem -"));
    }

    #[test]
    fn every_package_is_read_and_files_uci_cant_load_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("etc/config");
        std::fs::create_dir_all(config.join("subdir")).unwrap();
        let network = "config interface 'lan'\n\toption proto 'static'\n";
        std::fs::write(config.join("network"), network).unwrap();
        std::fs::write(config.join("dhcp-opkg"), "").unwrap();
        std::fs::write(config.join("network.bak"), "x").unwrap();
        std::fs::write(config.join("odd"), [0xff, 0xfe]).unwrap();
        let read = all();
        let got = read.parse(run(&read.request, root.path())).unwrap();
        assert_eq!(
            got.keys().collect::<Vec<_>>(),
            ["dhcp-opkg", "network", "odd"]
        );
        assert_eq!(got["network"].as_deref(), Ok(network));
        assert_eq!(got["dhcp-opkg"].as_deref(), Ok(""));
        assert!(got["odd"].as_ref().unwrap_err().contains("not UTF-8"));
    }

    #[test]
    fn an_empty_config_directory_reads_as_nothing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("etc/config")).unwrap();
        let read = all();
        assert!(
            read.parse(run(&read.request, root.path()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn services_report_enabled_disabled_and_absent() {
        let root = tempfile::tempdir().unwrap();
        let init = root.path().join("etc/init.d");
        std::fs::create_dir_all(&init).unwrap();
        for (name, status) in [("dnsmasq", 0), ("firewall", 1)] {
            let path = init.join(name);
            std::fs::write(&path, format!("#!/bin/sh\nexit {status}\n")).unwrap();
            let mut mode = std::fs::metadata(&path).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
            std::fs::set_permissions(&path, mode).unwrap();
        }
        let read = services(&["dnsmasq", "firewall", "odhcpd"]).unwrap();
        let got = read.parse(run(&read.request, root.path())).unwrap();
        assert_eq!(got["dnsmasq"], ServiceState::On);
        assert_eq!(got["firewall"], ServiceState::Off);
        assert_eq!(got["odhcpd"], ServiceState::Absent);
    }

    #[test]
    fn names_are_checked_and_bad_output_is_refused() {
        assert!(packages(&["../shadow"]).is_err());
        assert!(services(&["a b"]).is_err());
        assert!(parse_packages(b"network 10\nshort").is_err());
        assert!(parse_packages(b"network\n").is_err());
        assert!(parse_services(b"dnsmasq maybe\n").is_err());
    }
}
