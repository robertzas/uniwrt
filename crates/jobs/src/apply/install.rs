//! The apply's install step (D-076): the feed packages a node's configuration needs and it
//! lacks (SQM's, mwan3), installed with its package manager before anything is staged. The
//! services they bring are stopped and disabled at once, so the node runs as before until its
//! transaction switches them on, and a rollback leaves them off. A failure changes nothing
//! else: no transaction has started. And the transaction tool itself, brought to this
//! controller's version before anything is staged (D-081).

use std::time::Duration;

use uniwrt_model::{
    facts::PackageManager,
    job::{JobError, JobErrorCode, JobNodeState, StepName},
};
use uniwrt_node::{
    ExecRequest,
    txn::{self, Footprint},
};
use uniwrt_render::Requirement;

use super::run::Run;
use crate::{
    errors::{chain, last_line},
    progress::Progress,
};

/// How long refreshing the index or installing may take: slow mirrors on slow routers.
const TIMEOUT: Duration = Duration::from_secs(300);
/// The most output a package manager may print.
const MAX_OUTPUT: usize = 1 << 20;

/// The commands that install `packages`: refresh the index, then install.
pub(crate) fn commands(manager: PackageManager, packages: &[Requirement]) -> Vec<Vec<String>> {
    let (tool, add) = match manager {
        PackageManager::Opkg => ("opkg", "install"),
        PackageManager::Apk => ("apk", "add"),
    };
    let mut install = vec![tool.to_owned(), add.to_owned()];
    install.extend(packages.iter().map(|r| r.package.to_owned()));
    vec![vec![tool.to_owned(), "update".to_owned()], install]
}

/// The commands that keep a newly installed service off until the transaction starts it.
pub(crate) fn quiet(packages: &[Requirement]) -> Vec<Vec<String>> {
    packages
        .iter()
        .filter_map(|r| r.service)
        .flat_map(|service| {
            let script = format!("/etc/init.d/{service}");
            [
                vec![script.clone(), "stop".to_owned()],
                vec![script, "disable".to_owned()],
            ]
        })
        .collect()
}

fn request(argv: Vec<String>) -> ExecRequest {
    let mut request = ExecRequest::new(argv);
    request.timeout = TIMEOUT;
    request.max_output = MAX_OUTPUT;
    request
}

impl Run<'_> {
    /// Brings the transaction tool on every node that changes to this controller's version: a
    /// node keeps the one it got when management was turned on, which may lack a newer
    /// command (`bounce`, D-081). Nothing is staged yet.
    pub(crate) async fn update_tools(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        let footprint = Footprint::get();
        for i in self.active() {
            let live = &self.lives[i];
            if live.follows() {
                continue;
            }
            let updated = async {
                let status = live.tool(&txn::status()).await?;
                if status.is_some_and(|status| status.version == footprint.version()) {
                    return Ok(());
                }
                live.tool(&footprint.install()).await.map(|_| ())
            }
            .await;
            if let Err(err) = updated {
                let why = format!("Cannot update the transaction tool: {}", chain(&err));
                progress
                    .node(live.node.id, JobNodeState::Failed, &why)
                    .await;
                self.not_applied(progress).await;
                return Err(JobError {
                    code: JobErrorCode::NodeFailed,
                    message: format!("Nothing changed. {}: {why}", live.node.name),
                });
            }
        }
        Ok(())
    }

    /// Installs what each node lacks, one node at a time; nothing is staged yet.
    pub(crate) async fn install(&mut self, progress: &mut Progress) -> Result<(), JobError> {
        let wanted: Vec<usize> = (0..self.lives.len())
            .filter(|&i| self.lives[i].installs().is_some())
            .collect();
        if wanted.is_empty() {
            progress.skip(StepName::Install, "Nothing to install").await;
            return Ok(());
        }
        progress.start(StepName::Install).await;
        let mut installed = Vec::new();
        for i in wanted {
            let live = &self.lives[i];
            let Some((manager, packages)) = live.installs() else {
                continue;
            };
            let names: Vec<&str> = packages.iter().map(|r| r.package).collect();
            let names = names.join(", ");
            let id = live.node.id;
            progress
                .node(id, JobNodeState::Pending, format!("Installing {names}"))
                .await;
            let mut result = Ok(());
            for argv in commands(*manager, packages) {
                let what = argv.join(" ");
                let outcome = live.exec(request(argv)).await;
                result = match outcome {
                    Ok(output) if output.status.success() => Ok(()),
                    Ok(output) => {
                        let text = if output.stderr.is_empty() {
                            &output.stdout
                        } else {
                            &output.stderr
                        };
                        Err(format!("{what} failed: {}", last_line(text)))
                    }
                    Err(err) => Err(format!("{what} failed: {}", chain(&err))),
                };
                if result.is_err() {
                    break;
                }
            }
            if let Err(why) = result {
                progress.node(id, JobNodeState::Failed, &why).await;
                self.not_applied(progress).await;
                let done = if installed.is_empty() {
                    String::new()
                } else {
                    format!(" (installed on {} first)", installed.join(", "))
                };
                return Err(JobError {
                    code: JobErrorCode::NodeFailed,
                    message: format!(
                        "No configuration changed{done}. {}: {why}. Does the gateway reach the internet and its package feeds?",
                        live.node.name
                    ),
                });
            }
            for argv in quiet(packages) {
                // A service that won't stop or disable is switched by the transaction anyway.
                let _ = live.exec(request(argv)).await;
            }
            progress
                .node(id, JobNodeState::Pending, format!("Installed {names}"))
                .await;
            installed.push(live.node.name.clone());
            // A later transaction is planned from what the node holds then (D-081).
            let packages = packages.clone();
            self.lives[i].reading.installed(&packages);
        }
        progress
            .done(Some(format!("Installed on {}", installed.join(", "))))
            .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQM: [Requirement; 2] = [
        Requirement {
            package: "sqm-scripts",
            service: Some("sqm"),
        },
        Requirement {
            package: "kmod-sched-cake",
            service: None,
        },
    ];

    #[test]
    fn each_package_manager_refreshes_then_installs() {
        assert_eq!(
            commands(PackageManager::Apk, &SQM),
            [
                vec!["apk", "update"],
                vec!["apk", "add", "sqm-scripts", "kmod-sched-cake"]
            ]
        );
        assert_eq!(
            commands(PackageManager::Opkg, &SQM)[1],
            ["opkg", "install", "sqm-scripts", "kmod-sched-cake"]
        );
        assert_eq!(
            quiet(&SQM),
            [
                vec!["/etc/init.d/sqm", "stop"],
                vec!["/etc/init.d/sqm", "disable"]
            ]
        );
    }
}
