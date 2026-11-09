//! How adoption logs in, and root's password on a node that had none (features/adoption.md,
//! Identity, credentials, trust). Factory-default OpenWrt lets root in without a password, so
//! anyone on its LAN could log in: adoption gives it a random one, which the vault keeps.
//!
//! The password is sealed in the vault under the node's MAC before `passwd` runs, and moved to
//! the node's own name once it is recorded, so no failure in between can lose it: a retried
//! adoption logs in with the key installed by then and moves the password it finds.

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::job::JobError;
use uniwrt_node::{Credential, root_password};
use uniwrt_store::VaultRepo as _;
use zeroize::Zeroizing;

use super::AdoptRequest;
use crate::{
    Dial, Remote,
    errors::{connect_error, internal, node_error},
    runner::Shared,
};

/// How the first login got in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Login {
    /// The controller's key: the node trusts it already.
    Key,
    /// The root password the user gave.
    Password,
    /// No password: root has none (factory defaults).
    Blank,
}

impl Login {
    pub(crate) fn detail(self) -> &'static str {
        match self {
            Self::Key => "Logged in with the controller's key",
            Self::Password => "Logged in with the root password",
            Self::Blank => "Logged in as root, which has no password (factory defaults)",
        }
    }
}

/// The vault's name for root's password on node `id`.
pub(crate) fn vault_name(id: NodeId) -> String {
    format!("node/{id}/root_password")
}

/// Where the password waits until the node is recorded.
fn pending_name(mac: MacAddr) -> String {
    format!("adopt/{mac}/root_password")
}

/// Logs in with the password the user gave (an empty one meaning none), or else with the
/// controller's key, falling back to no password when the node refuses the key: a factory
/// node then adopts with the password field left empty. A refusal of both reports the key's.
pub(crate) async fn first(
    shared: &Shared,
    dial: &dyn Dial,
    request: &AdoptRequest,
) -> Result<(Box<dyn Remote>, Login), JobError> {
    let connect = |credential: Credential| async move {
        dial.connect(&request.target, &request.host_key, &credential)
            .await
    };
    let blank = || Credential::Password(Zeroizing::new(String::new()));
    match &request.password {
        Some(password) if password.is_empty() => connect(blank())
            .await
            .map(|remote| (remote, Login::Blank))
            .map_err(|err| connect_error(&err)),
        Some(password) => connect(Credential::Password(password.clone()))
            .await
            .map(|remote| (remote, Login::Password))
            .map_err(|err| connect_error(&err)),
        None => match connect(Credential::Key(shared.key.clone())).await {
            Ok(remote) => Ok((remote, Login::Key)),
            Err(err @ uniwrt_node::Error::AuthRejected { .. }) => connect(blank())
                .await
                .map(|remote| (remote, Login::Blank))
                .map_err(|_| connect_error(&err)),
            Err(err) => Err(connect_error(&err)),
        },
    }
}

/// Gives root a random password, sealed in the vault first; returns the step's detail.
pub(crate) async fn set_password(
    shared: &Shared,
    remote: &dyn Remote,
    mac: MacAddr,
) -> Result<String, JobError> {
    let password =
        root_password::generate().map_err(|err| internal("cannot make a root password", &err))?;
    shared
        .vault
        .put_secret(
            &pending_name(mac),
            Zeroizing::new(password.as_bytes().to_vec()),
        )
        .await
        .map_err(|err| internal("cannot keep the root password", &err))?;
    remote
        .exec(root_password::set(&password))
        .await
        .and_then(|output| output.check("passwd"))
        .map_err(|err| node_error(&err, "Cannot set the root password"))?;
    Ok("Root had no password: it has a random one now, kept in the vault".to_owned())
}

/// Moves a password [`set_password`] kept for the node with `mac` to node `id`'s name; says
/// whether there was one.
pub(crate) async fn keep_pending(
    shared: &Shared,
    mac: MacAddr,
    id: NodeId,
) -> uniwrt_store::Result<bool> {
    let pending = pending_name(mac);
    let Some(password) = shared.vault.secret(&pending).await? else {
        return Ok(false);
    };
    shared.vault.put_secret(&vault_name(id), password).await?;
    shared.vault.delete_secret(&pending).await?;
    Ok(true)
}
