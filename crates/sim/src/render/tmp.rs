//! A node's `/tmp`, in RAM: what the controller uploads there (the DNS blocklist, D-133), gone
//! when the node boots again; and the commands that tend it, answered as OpenWrt's busybox and
//! procd would (`mkdir -p`, `rm -f`, `ubus call service signal`, `/etc/init.d/<service>
//! restart`).

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_node::ExecOutput;

use super::commands::{failed, ok};

/// The files and directories under `/tmp` since the node booted.
#[derive(Debug, Default)]
pub(crate) struct Tmp {
    booted_ms: i64,
    dirs: BTreeSet<String>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Tmp {
    /// The file at `path`, if it was written since the node booted at `booted_ms`.
    pub(crate) fn file(&self, booted_ms: i64, path: &str) -> Option<&[u8]> {
        (self.booted_ms == booted_ms)
            .then(|| self.files.get(path).map(Vec::as_slice))
            .flatten()
    }

    /// Writes `path` whole, as the agent's upload does; its directory must exist.
    pub(crate) fn write(
        &mut self,
        booted_ms: i64,
        path: &str,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let parent = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        self.since(booted_ms);
        if parent != "/tmp" && !self.dirs.contains(parent) {
            return Err(format!("{parent}: No such file or directory"));
        }
        self.files.insert(path.to_owned(), data);
        Ok(())
    }

    /// Forgets what was there before a boot.
    fn since(&mut self, booted_ms: i64) {
        if self.booted_ms != booted_ms {
            *self = Self {
                booted_ms,
                ..Self::default()
            };
        }
    }
}

/// Whether `path` lies under `/tmp`, without climbing out of it.
pub(crate) fn under_tmp(path: &str) -> bool {
    path.strip_prefix("/tmp/")
        .is_some_and(|rest| !rest.is_empty() && !rest.split('/').any(|p| p.is_empty() || p == ".."))
}

/// Answers the commands that tend `/tmp` and restart services; `None` for any other.
pub(crate) fn exec(
    argv: &[&str],
    tmp: &mut Tmp,
    booted_ms: i64,
    services: &mut BTreeMap<String, bool>,
) -> Option<ExecOutput> {
    match argv {
        ["mkdir", "-p", dirs @ ..] if !dirs.is_empty() && dirs.iter().all(|d| under_tmp(d)) => {
            tmp.since(booted_ms);
            for dir in dirs {
                // `-p` makes the parents too.
                let mut at = String::from("/tmp");
                for part in dir.trim_start_matches("/tmp/").split('/') {
                    at.push('/');
                    at.push_str(part);
                    tmp.dirs.insert(at.clone());
                }
            }
            Some(ok(Vec::new()))
        }
        ["rm", "-f", paths @ ..] if paths.iter().all(|p| under_tmp(p)) => {
            tmp.since(booted_ms);
            for path in paths {
                tmp.files.remove(*path);
            }
            Some(ok(Vec::new()))
        }
        // procd signals the service's running instances; one that doesn't run is no error.
        ["ubus", "call", "service", "signal", message] => {
            let message: serde_json::Value = serde_json::from_str(message).ok()?;
            let known = message["name"]
                .as_str()
                .is_some_and(|name| services.contains_key(name));
            Some(if known {
                ok(Vec::new())
            } else {
                failed(4, "Command failed: Not found\n".to_owned())
            })
        }
        [script, "restart"] => {
            let name = script.strip_prefix("/etc/init.d/")?;
            let running = services.get_mut(name)?;
            *running = true;
            Some(ok(Vec::new()))
        }
        _ => None,
    }
}
