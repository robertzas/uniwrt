//! Taking a node's configuration backup over a login and keeping it, sealed (D-126): the
//! Backup step of the backup, restore, apply and upgrade jobs.

use uniwrt_core::NodeId;
use uniwrt_model::{inventory::BackupReason, job::JobError};
use uniwrt_node::{ExecRequest, archive};
use uniwrt_store::{BackupRepo as _, Kept};
use zeroize::Zeroizing;

use crate::{
    Remote,
    adopt::{BACKUP_TIMEOUT, MAX_BACKUP},
    errors::{internal, node_error},
    runner::Shared,
};

/// The tarball `sysupgrade -b` writes on `remote`, checked to be gzip.
pub(crate) async fn read(remote: &dyn Remote) -> Result<Zeroizing<Vec<u8>>, JobError> {
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
            code: uniwrt_model::job::JobErrorCode::NodeFailed,
            message: "sysupgrade -b did not write a gzip tarball".to_owned(),
        });
    }
    Ok(tarball)
}

/// The digest of the files `tarball` holds (D-126), when it can be read.
pub(crate) fn content(tarball: &[u8]) -> Option<String> {
    archive::content_digest(tarball).ok()
}

/// Takes `node`'s backup over `remote`, seals and keeps it; says what was kept, for people.
pub(crate) async fn take(
    shared: &Shared,
    node: NodeId,
    remote: &dyn Remote,
    reason: BackupReason,
) -> Result<String, JobError> {
    let tarball = read(remote).await?;
    let sealed = shared
        .vault
        .seal_backup(reason, shared.clock.now(), &tarball, content(&tarball))
        .map_err(|err| internal("cannot seal the backup", &err))?;
    let kept = shared
        .store
        .keep_backup(node, sealed)
        .await
        .map_err(|err| internal("keeping the backup", &err))?;
    let kib = tarball.len().div_ceil(1024);
    Ok(match kept {
        Kept::New(_) => format!("{kib} KiB, sealed on the controller"),
        Kept::Unchanged(_) => {
            format!("Unchanged since its last backup ({kib} KiB), which stays the newest")
        }
    })
}
