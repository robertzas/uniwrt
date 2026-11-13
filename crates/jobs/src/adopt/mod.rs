//! The adopt job (features/adoption.md, Path A): log in with the host key the user confirmed,
//! check the node, install the controller's key, read its inventory, take a backup, record it
//! all, and start watching. In observe mode, `authorized_keys` is the only thing it changes on
//! the node (directive 8), but for root's password on a node that had none ([`login`]); the
//! agent runs from `/tmp`. A factory-default device on the gateway's adoption port takes
//! [`port`]'s path instead (Path C): the same steps through the gateway, then staged, managed
//! and confirmed at its lease on the LAN.

pub(crate) mod check;
mod login;
mod port;
#[cfg(test)]
mod port_tests;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use tracing::{info, warn};
use uniwrt_model::{
    facts::NodeFacts,
    inventory::{BackupReason, Inventory},
    job::{JobError, JobErrorCode, StepName},
    node::{NewNode, NodeRole},
};
use uniwrt_node::{Credential, ExecRequest, HostKey, Target, pkg, proto::ops::Set};
use uniwrt_store::{Adoption, NodeRepo as _, VaultRepo as _};
use zeroize::Zeroizing;

use self::login::Login;
pub use self::port::PortPath;
pub(crate) use self::port::STEPS as PORT_STEPS;
use crate::{
    Dial, Remote,
    errors::{chain, internal, node_error, unsupported},
    progress::Progress,
    runner::Shared,
};

/// The adopt job's steps, in order.
pub(crate) const STEPS: [StepName; 9] = [
    StepName::Connect,
    StepName::Check,
    StepName::InstallKey,
    StepName::VerifyKey,
    StepName::SetPassword,
    StepName::Inventory,
    StepName::Backup,
    StepName::Record,
    StepName::Watch,
];

/// Appends the controller's key (`$1`) to dropbear's `authorized_keys` unless it is there,
/// keeping the file's last line intact; prints `present` or `added`.
pub(crate) const INSTALL_KEY: &str = r#"f=/etc/dropbear/authorized_keys
if [ -f "$f" ] && grep -qxF "$1" "$f"; then echo present; exit 0; fi
umask 077
if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >>"$f"; fi
printf '%s\n' "$1" >>"$f" && echo added"#;

/// `sysupgrade -b` tarballs are tens of KiB; a node with large files in its backup list may
/// reach a few MiB.
const MAX_BACKUP: usize = 32 << 20;
const BACKUP_TIMEOUT: Duration = Duration::from_secs(120);

/// What adopting a node takes from the user.
pub struct AdoptRequest {
    pub target: Target,
    /// The host key the user confirmed; the node must present it.
    pub host_key: HostKey,
    /// The root password, to install the controller's key; empty for a node without one
    /// (factory defaults). `None` logs in with the controller's key, or without a password when
    /// the node refuses the key.
    pub password: Option<Zeroizing<String>>,
    /// Keep the password in the vault, for emergencies (features/adoption.md).
    pub remember_password: bool,
    /// The node's display name; its hostname when `None`.
    pub name: Option<String>,
    /// Its role; inferred from its interfaces when `None`.
    pub role: Option<NodeRole>,
    /// What a zero-touch device said it is when it called home (D-102): the node must match,
    /// and root's password, the image's random one that nobody knows, is replaced.
    pub ztp: Option<ZtpClaim>,
    /// A factory-default device on the gateway's adoption port (Path C, D-107): reached through
    /// the gateway, staged, and confirmed at its lease on the LAN; `target` is 192.168.1.1.
    pub port: Option<PortPath>,
}

/// A zero-touch device's hello, as far as adoption checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZtpClaim {
    /// One of its MAC addresses.
    pub mac: uniwrt_core::MacAddr,
    pub board_name: String,
}

pub(crate) async fn run(shared: Arc<Shared>, request: AdoptRequest, mut progress: Progress) {
    // Never closed.
    let _permit = shared.adoptions.acquire().await;
    let result = adopt(&shared, &request, &mut progress).await;
    match &result {
        Ok(()) => info!("adopted"),
        Err(err) => info!(code = ?err.code, "adoption failed: {}", err.message),
    }
    progress.finish(result.err()).await;
}

/// What the steps on the node found, for the steps that record it.
struct Found {
    facts: NodeFacts,
    mac: uniwrt_core::MacAddr,
    inventory: Inventory,
    backup: uniwrt_store::SealedBackup,
}

async fn adopt(
    shared: &Arc<Shared>,
    request: &AdoptRequest,
    progress: &mut Progress,
) -> Result<(), JobError> {
    if let Some(path) = &request.port {
        return port::adopt(shared, request, path, progress).await;
    }
    let Some(dial) = &shared.dial else {
        return Err(internal("no way to reach nodes", &"no dial"));
    };
    let (found, remote) = on_node(shared, dial.as_ref(), request, progress).await?;
    remote.close().await;
    let host = request.target.host.clone();
    let facts = found.facts.clone();
    let node = record(shared, request, found, host, false, progress).await?;
    watch(shared, request, node, facts, progress).await;
    Ok(())
}

/// Records the node at `host`, managed or in observe mode, and moves its root password to its
/// name.
async fn record(
    shared: &Shared,
    request: &AdoptRequest,
    found: Found,
    host: String,
    managed: bool,
    progress: &mut Progress,
) -> Result<uniwrt_model::node::Node, JobError> {
    progress.start(StepName::Record).await;
    let Found {
        facts,
        mac,
        inventory,
        backup,
    } = found;
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| Some(facts.board.hostname.as_str()).filter(|name| !name.is_empty()))
        .unwrap_or(facts.board.model.as_str())
        .to_owned();
    let role = request.role.unwrap_or_else(|| check::role(&facts));
    let adoption = Adoption {
        node: NewNode {
            name,
            mac,
            board_name: facts.board.board_name.clone(),
            host,
            port: request.target.port,
            role,
        },
        pin: request.host_key.pin(shared.clock.now()),
        facts,
        inventory,
        backup: Some(backup),
    };
    let mut node = shared
        .store
        .adopt_node(adoption)
        .await
        .map_err(|err| match err {
            uniwrt_store::Error::Conflict { .. } => already_adopted(None),
            err => internal("cannot record the node", &err),
        })?;
    progress.set_node(node.id);
    if managed {
        node = shared
            .store
            .set_node_mode(node.id, uniwrt_model::node::NodeMode::Managed)
            .await
            .map_err(|err| internal("recording the node's mode", &err))?
            .ok_or_else(|| internal("recording the node's mode", &"the node is gone"))?;
    }
    let mut detail = format!(
        "Recorded as {}{}",
        node.name,
        if managed {
            ", managed"
        } else {
            " in observe mode"
        }
    );
    match login::keep_pending(shared, node.mac, node.id).await {
        Ok(true) => detail.push_str("; its root password is kept in the vault"),
        Ok(false) => {}
        Err(err) => {
            warn!(%err, "cannot move the node's root password");
            detail.push_str("; its root password is kept in the vault under its MAC address");
        }
    }
    if let (true, Some(password)) = (
        request.remember_password,
        request
            .password
            .as_ref()
            .filter(|password| !password.is_empty()),
    ) {
        let secret = Zeroizing::new(password.as_bytes().to_vec());
        if let Err(err) = shared
            .vault
            .put_secret(&login::vault_name(node.id), secret)
            .await
        {
            warn!(%err, "cannot keep the node's password");
            detail.push_str("; the password could not be kept");
        } else {
            detail.push_str("; the password is kept in the vault");
        }
    }
    progress.done(Some(detail)).await;
    Ok(node)
}

async fn watch(
    shared: &Shared,
    request: &AdoptRequest,
    node: uniwrt_model::node::Node,
    facts: NodeFacts,
    progress: &mut Progress,
) {
    progress.start(StepName::Watch).await;
    shared
        .watch
        .watch(node, request.host_key.clone(), facts)
        .await;
    progress.done(None).await;
}

/// The steps on the node through `dial`, from logging in to the backup; returns the login.
async fn on_node(
    shared: &Shared,
    dial: &dyn Dial,
    request: &AdoptRequest,
    progress: &mut Progress,
) -> Result<(Found, Box<dyn Remote>), JobError> {
    progress.start(StepName::Connect).await;
    let (first, login) = login::first(shared, dial, request).await?;
    progress.done(Some(login.detail().to_owned())).await;

    let checked = match check(shared, request, progress, first.as_ref()).await {
        Ok(checked) => checked,
        Err(err) => {
            first.close().await;
            return Err(err);
        }
    };
    let remote = match key_login(shared, dial, request, progress, first.as_ref(), login).await {
        Ok(Some(second)) => {
            first.close().await;
            second
        }
        Ok(None) => first,
        Err(err) => {
            first.close().await;
            return Err(err);
        }
    };
    if login == Login::Blank || request.ztp.is_some() {
        progress.start(StepName::SetPassword).await;
        let blank = login == Login::Blank;
        match login::set_password(shared, remote.as_ref(), checked.mac, blank).await {
            Ok(detail) => progress.done(Some(detail)).await,
            Err(err) => {
                remote.close().await;
                return Err(err);
            }
        }
    } else {
        progress
            .skip(StepName::SetPassword, "Root has a password already")
            .await;
    }
    match inspect(shared, progress, remote.as_ref(), checked).await {
        Ok(found) => Ok((found, remote)),
        Err(err) => {
            remote.close().await;
            Err(err)
        }
    }
}

/// What the check found: the facts, the node's identity, and notes for the inventory step.
struct Checked {
    facts: NodeFacts,
    mac: uniwrt_core::MacAddr,
    notes: Vec<String>,
}

/// Checks the release, then collects the facts with the agent and checks the resources and the
/// identity, before anything changes on the node.
async fn check(
    shared: &Shared,
    request: &AdoptRequest,
    progress: &mut Progress,
    remote: &dyn Remote,
) -> Result<Checked, JobError> {
    progress.start(StepName::Check).await;
    let release = text(
        remote,
        ["cat", "/etc/openwrt_release"],
        "Cannot read the release",
    )
    .await?;
    let release = check::release(&release).map_err(unsupported)?;
    let collected = remote
        .collect(&Set::ALL)
        .await
        .map_err(|err| node_error(&err, "The agent cannot collect the node's facts"))?;
    let mut facts = NodeFacts::default();
    let errors = uniwrt_node::facts::apply(&collected, &mut facts);
    facts.collected_at = shared.clock.now();
    if let Some(err) = errors
        .iter()
        .find(|err| matches!(err.set, Set::Board | Set::System))
    {
        return Err(JobError {
            code: JobErrorCode::NodeFailed,
            message: format!("Cannot tell what the node is: {err}"),
        });
    }
    let warnings = check::resources(&facts).map_err(unsupported)?;
    let mac = check::identity(&facts)
        .ok_or_else(|| unsupported("Cannot tell the node's MAC address".to_owned()))?;
    if let Some(claim) = &request.ztp {
        check::claim(&facts, mac, claim)?;
    }
    let adopted = shared
        .store
        .nodes()
        .await
        .map_err(|err| internal("cannot read the nodes", &err))?;
    if let Some(node) = adopted.iter().find(|node| node.mac == mac) {
        return Err(already_adopted(Some(&node.name)));
    }
    let board = &facts.board;
    let mut detail = format!(
        "{release} on {} ({}, {})",
        board.model, board.board_name, board.release.arch
    );
    for warning in &warnings {
        detail.push_str("; ");
        detail.push_str(warning);
    }
    progress.done(Some(detail)).await;
    Ok(Checked {
        notes: errors.iter().map(ToString::to_string).collect(),
        facts,
        mac,
    })
}

/// Installs the controller's key and logs in with it when the first login didn't use it;
/// returns that second login.
async fn key_login(
    shared: &Shared,
    dial: &dyn Dial,
    request: &AdoptRequest,
    progress: &mut Progress,
    remote: &dyn Remote,
    login: Login,
) -> Result<Option<Box<dyn Remote>>, JobError> {
    if login == Login::Key {
        let detail = "The node trusts the controller's key already";
        progress.skip(StepName::InstallKey, detail).await;
        progress.skip(StepName::VerifyKey, detail).await;
        return Ok(None);
    }

    progress.start(StepName::InstallKey).await;
    let line = shared
        .key
        .authorized_key()
        .map_err(|err| internal("cannot encode the controller's key", &err))?;
    let installed = text(
        remote,
        ["sh", "-c", INSTALL_KEY, "sh", line.as_str()],
        "Cannot install the controller's key",
    )
    .await?;
    progress
        .done(Some(
            if installed.trim() == "present" {
                "The controller's key was in /etc/dropbear/authorized_keys already"
            } else {
                "Added the controller's key to /etc/dropbear/authorized_keys"
            }
            .to_owned(),
        ))
        .await;

    progress.start(StepName::VerifyKey).await;
    let second = dial
        .connect(
            &request.target,
            &request.host_key,
            &Credential::Key(shared.key.clone()),
        )
        .await
        .map_err(|err| JobError {
            code: JobErrorCode::NodeFailed,
            message: format!(
                "The node does not accept the controller's key it was given (is key login \
                 disabled in dropbear?): {}",
                chain(&err)
            ),
        })?;
    progress
        .done(Some("Logged in with the controller's key".to_owned()))
        .await;
    Ok(Some(second))
}

/// Reads the node's packages and takes the backup.
async fn inspect(
    shared: &Shared,
    progress: &mut Progress,
    remote: &dyn Remote,
    checked: Checked,
) -> Result<Found, JobError> {
    let Checked {
        facts,
        mac,
        mut notes,
    } = checked;
    progress.start(StepName::Inventory).await;
    let packages = if let Some(manager) = facts.capabilities.package_manager {
        let mut outputs = Vec::new();
        for argv in pkg::inventory(manager) {
            match text_argv(remote, &argv, "Cannot read the packages").await {
                Ok(output) => outputs.push(output),
                Err(err) => notes.push(err.message),
            }
        }
        pkg::parse(manager, &outputs)
    } else {
        notes.push("No package manager found".to_owned());
        Vec::new()
    };
    let explicit = packages.iter().filter(|package| package.explicit).count();
    let mut detail = format!(
        "{} packages, {explicit} of them installed on purpose",
        packages.len()
    );
    for note in &notes {
        detail.push_str("; ");
        detail.push_str(note);
    }
    progress.done(Some(detail)).await;
    let inventory = Inventory {
        taken_at: facts.collected_at,
        packages,
    };

    progress.start(StepName::Backup).await;
    let output = remote
        .exec(
            ExecRequest::new(["sysupgrade", "-b", "-"])
                .timeout(BACKUP_TIMEOUT)
                .max_output(MAX_BACKUP),
        )
        .await
        .and_then(|output| output.check("sysupgrade -b"))
        .map_err(|err| node_error(&err, "Cannot take a backup"))?;
    let tarball = Zeroizing::new(output.stdout);
    if !tarball.starts_with(&[0x1f, 0x8b]) {
        return Err(JobError {
            code: JobErrorCode::NodeFailed,
            message: "sysupgrade -b did not write a gzip tarball".to_owned(),
        });
    }
    let backup = shared
        .vault
        .seal_backup(BackupReason::Adoption, shared.clock.now(), &tarball)
        .map_err(|err| internal("cannot seal the backup", &err))?;
    progress
        .done(Some(format!(
            "{} KiB, sealed on the controller",
            tarball.len().div_ceil(1024)
        )))
        .await;
    Ok(Found {
        facts,
        mac,
        inventory,
        backup,
    })
}

/// Runs a command that must succeed and returns its output as text.
async fn text<const N: usize>(
    remote: &dyn Remote,
    argv: [&str; N],
    what: &str,
) -> Result<String, JobError> {
    text_argv(remote, &argv, what).await
}

async fn text_argv(remote: &dyn Remote, argv: &[&str], what: &str) -> Result<String, JobError> {
    let output = remote
        .exec(ExecRequest::new(argv.iter().copied()))
        .await
        .and_then(|output| output.check(argv.first().copied().unwrap_or("a command")))
        .map_err(|err| node_error(&err, what))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn already_adopted(name: Option<&str>) -> JobError {
    JobError {
        code: JobErrorCode::AlreadyAdopted,
        message: match name {
            Some(name) => format!("The node is adopted already, as {name}"),
            None => "The node is adopted already".to_owned(),
        },
    }
}
