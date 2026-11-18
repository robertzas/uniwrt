//! What the gateway is sent ([`Action`], decided from what it reports) and the commands that
//! send it, as argv arrays of constants.

use std::time::Duration;

use bytes::Bytes;
use uniwrt_model::{
    facts::BlocklistFacts,
    site::blocking::{BLOCKLIST_CONF, BLOCKLIST_FILE, CONF_DIR},
};
use uniwrt_node::{ExecRequest, ExitStatus, NodeLink};

/// procd's `signal` for dnsmasq's instances: `SIGHUP`, which re-reads the servers-file (checked
/// on OpenWrt 24.10 and 25.12: the same process then answers for the new domains).
const SIGHUP: &str = r#"{"name":"dnsmasq","signal":1}"#;

/// What the gateway needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    /// The compiled list; `restart` when dnsmasq must be restarted to load it (the conf file is
    /// missing, or the list was: a dnsmasq told to read a missing file doesn't run).
    Push { restart: bool },
    /// The files gone (blocking is off).
    Remove,
}

/// What the gateway reporting `facts` needs: with blocking on, the file `sha256` (`None` while
/// it isn't compiled yet); with it off, none of the files.
pub(super) fn action(facts: &BlocklistFacts, on: bool, sha256: Option<&str>) -> Option<Action> {
    if !on {
        return (facts.sha256.is_some() || facts.conf).then_some(Action::Remove);
    }
    let sha256 = sha256?;
    let restart = !facts.conf || facts.sha256.is_none();
    (restart || facts.sha256.as_deref() != Some(sha256)).then_some(Action::Push { restart })
}

/// Sends the list `text`, and loads it.
pub(super) async fn push<L: NodeLink>(
    link: &L,
    text: Bytes,
    restart: bool,
    timeout: Duration,
) -> Result<(), String> {
    run(link, &["mkdir", "-p", CONF_DIR], timeout).await?;
    upload(link, BLOCKLIST_FILE, text, timeout).await?;
    let restart_dnsmasq = ["/etc/init.d/dnsmasq", "restart"];
    if restart {
        let conf = Bytes::from(format!("servers-file={BLOCKLIST_FILE}\n"));
        upload(link, BLOCKLIST_CONF, conf, timeout).await?;
        return run(link, &restart_dnsmasq, timeout).await;
    }
    let signal = ["ubus", "call", "service", "signal", SIGHUP];
    if run(link, &signal, timeout).await.is_err() {
        // procd answers "Not found" while dnsmasq doesn't run: a restart starts it.
        return run(link, &restart_dnsmasq, timeout).await;
    }
    Ok(())
}

/// Removes the files. dnsmasq no longer reads the directory: the apply that turned blocking
/// off restarted it without the `confdir`.
pub(super) async fn remove<L: NodeLink>(link: &L, timeout: Duration) -> Result<(), String> {
    run(link, &["rm", "-f", BLOCKLIST_CONF, BLOCKLIST_FILE], timeout).await
}

async fn upload<L: NodeLink>(
    link: &L,
    path: &str,
    data: Bytes,
    timeout: Duration,
) -> Result<(), String> {
    match tokio::time::timeout(timeout, link.upload(path, data, 0o644)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(format!("cannot write {path}: {err}")),
        Err(_) => Err(format!("writing {path} took over {}s", timeout.as_secs())),
    }
}

async fn run<L: NodeLink>(link: &L, argv: &[&str], timeout: Duration) -> Result<(), String> {
    let request = ExecRequest {
        argv: argv.iter().map(|&a| a.to_owned()).collect(),
        stdin: None,
        timeout,
        max_output: 4096,
    };
    let what = argv.join(" ");
    let output = link
        .exec(request)
        .await
        .map_err(|err| format!("{what}: {err}"))?;
    match output.status {
        ExitStatus::Code(0) => Ok(()),
        status => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("{what} failed ({status:?}): {}", stderr.trim()))
        }
    }
}

#[cfg(test)]
mod tests {
    use uniwrt_model::facts::BlocklistFacts;

    use super::{Action, action};

    fn facts(sha256: Option<&str>, conf: bool) -> BlocklistFacts {
        BlocklistFacts {
            sha256: sha256.map(str::to_owned),
            domains: sha256.map(|_| 1),
            conf,
        }
    }

    #[test]
    fn the_gateway_gets_what_it_lacks() {
        let push = |restart| Some(Action::Push { restart });
        assert_eq!(action(&facts(None, false), true, Some("a")), push(true));
        assert_eq!(
            action(&facts(Some("b"), true), true, Some("a")),
            push(false)
        );
        assert_eq!(action(&facts(Some("a"), true), true, Some("a")), None);
        // The conf file without the list, or the list without it: dnsmasq restarts.
        assert_eq!(action(&facts(None, true), true, Some("a")), push(true));
        assert_eq!(
            action(&facts(Some("a"), false), true, Some("a")),
            push(true)
        );
        // Nothing compiled yet: nothing to send.
        assert_eq!(action(&facts(None, false), true, None), None);
        // Off: whatever is there goes, and nothing is asked when nothing is there.
        assert_eq!(
            action(&facts(Some("a"), true), false, Some("a")),
            Some(Action::Remove)
        );
        assert_eq!(
            action(&facts(None, true), false, None),
            Some(Action::Remove)
        );
        assert_eq!(action(&facts(None, false), false, None), None);
    }
}
