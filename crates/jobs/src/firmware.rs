//! Firmware (M7.1a1, D-121): a node's packages read again before an upgrade is planned, and
//! kept as its inventory. Only reads: allowed in observe mode too.

use uniwrt_model::{
    facts::PackageManager,
    inventory::{Inventory, Package},
    job::JobError,
    node::Node,
};
use uniwrt_node::{ExecRequest, pkg};
use uniwrt_store::InventoryRepo as _;

use crate::{
    Remote,
    errors::{internal, node_error},
    manage,
    runner::Shared,
};

/// Logs in, reads the installed packages from the manager's database, and keeps them.
pub(crate) async fn packages(
    shared: &Shared,
    node: &Node,
    manager: PackageManager,
) -> Result<Inventory, JobError> {
    let remote = manage::login(shared, node).await?;
    let read = keep(shared, node, remote.as_ref(), manager).await;
    remote.close().await;
    read
}

/// Reads the installed packages over `remote` and keeps them (also after an upgrade, D-124).
pub(crate) async fn keep(
    shared: &Shared,
    node: &Node,
    remote: &dyn Remote,
    manager: PackageManager,
) -> Result<Inventory, JobError> {
    let mut outputs = Vec::new();
    for argv in pkg::inventory(manager) {
        let output = remote
            .exec(ExecRequest::new(argv.iter().copied()))
            .await
            .and_then(|output| output.check(argv[0]))
            .map_err(|err| node_error(&err, "Cannot read the packages"))?;
        outputs.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let packages: Vec<Package> = pkg::parse(manager, &outputs);
    let inventory = Inventory {
        taken_at: shared.clock.now(),
        packages,
    };
    shared
        .store
        .set_inventory(node.id, inventory.clone())
        .await
        .map_err(|err| internal("keeping the packages", &err))?;
    Ok(inventory)
}

#[cfg(test)]
mod tests {
    use uniwrt_model::{facts::PackageManager, job::JobState};
    use uniwrt_store::InventoryRepo as _;

    use crate::{
        Error,
        testing::{PASSWORD, setup},
    };

    #[tokio::test]
    async fn the_packages_are_read_again_and_kept() {
        let setup = setup(true).await;
        let (job, _) = setup.adopt(setup.request(Some(PASSWORD))).await;
        assert_eq!(job.state, JobState::Succeeded, "{job:#?}");
        let id = job.node.unwrap();
        let before = setup.store.inventory(id).await.unwrap().unwrap();
        let greps = |setup: &crate::testing::Setup| {
            setup
                .node
                .commands()
                .iter()
                .filter(|c| *c == "grep")
                .count()
        };
        let greps_before = greps(&setup);

        let read = setup
            .jobs
            .read_packages(id, PackageManager::Opkg)
            .await
            .unwrap();
        assert_eq!(read.packages, before.packages);
        assert!(read.taken_at >= before.taken_at);
        assert_eq!(setup.store.inventory(id).await.unwrap(), Some(read));
        assert_eq!(
            greps(&setup),
            greps_before + 1,
            "the status file read again"
        );
        assert!(matches!(
            setup
                .jobs
                .read_packages(uniwrt_core::NodeId::new(99), PackageManager::Opkg)
                .await,
            Err(Error::NoNode(_))
        ));
    }
}
