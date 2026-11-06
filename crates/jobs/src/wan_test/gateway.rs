//! The gateway as a WAN test holds it: what render needs, the trial's transaction, the facts
//! the checks read and the commands they run.

use std::{collections::BTreeMap, time::Duration};

use tokio::time::{Instant, sleep};
use uniwrt_model::{
    facts::NodeFacts,
    job::{JobError, JobErrorCode, JobNodeState, StepName},
    node::Node,
    site::{SiteConfig, node::ManagedPackage},
};
use uniwrt_node::{
    ExecOutput, ExecRequest,
    proto::ops::Set,
    txn::{self, TxnId, current},
};
use uniwrt_render::Target;

use super::{patch::patch, run::failed};
use crate::{
    Remote,
    apply::{plan, survey},
    errors::{chain, internal},
    progress::Progress,
    runner::Shared,
};

/// What the checks read of the facts.
const SETS: [Set; 2] = [Set::Interfaces, Set::Devices];

/// The gateway as the test holds it.
pub(crate) struct Gateway<'a> {
    pub(crate) shared: &'a Shared,
    node: &'a Node,
    pub(crate) remote: Box<dyn Remote>,
    pub(crate) facts: NodeFacts,
    current: BTreeMap<String, uniwrt_uci::Package>,
    /// The transaction applied and not rolled back yet.
    open: Option<TxnId>,
}

impl<'a> Gateway<'a> {
    /// Reads what render needs: the managed packages as the gateway holds them, and its facts.
    pub(crate) async fn read(
        shared: &'a Shared,
        node: &'a Node,
        remote: Box<dyn Remote>,
    ) -> Result<Self, JobError> {
        let refused = |why: String| failed(JobErrorCode::Refused, why);
        let packages = current::packages(&plan::packages()).map_err(|e| refused(e.to_string()))?;
        let files = remote
            .exec(packages.request.clone())
            .await
            .and_then(|output| packages.parse(output))
            .map_err(|err| refused(format!("Cannot read its configuration: {}", chain(&err))))?;
        let mut current = BTreeMap::new();
        for (name, text) in files {
            let Some(text) = text else { continue };
            let package = uniwrt_uci::parse(&name, &text)
                .map_err(|err| refused(format!("Its /etc/config/{name} doesn't parse: {err}")))?;
            current.insert(name, package);
        }
        let facts = survey::collect(shared, remote.as_ref(), &Set::ALL, &survey::RENDER_SETS)
            .await
            .map_err(refused)?;
        if facts.board.layout.wan.is_none() {
            return Err(refused(
                "The gateway has no WAN port: its board.json names none".to_owned(),
            ));
        }
        Ok(Self {
            shared,
            node,
            remote,
            facts,
            current,
            open: None,
        })
    }

    /// The gateway's `network` with `site`'s WAN in place of its own.
    pub(crate) fn render(
        &self,
        site: &SiteConfig,
        secrets: &survey::VaultSecrets,
    ) -> Result<String, JobError> {
        let target = Target {
            node: self.node.id,
            role: self.node.role,
            facts: &self.facts,
            current: &self.current,
            controller: None,
        };
        let rendered = uniwrt_render::render(site, secrets, target).map_err(|issues| {
            let issues: Vec<String> = issues.into_iter().map(|i| i.message).collect();
            failed(JobErrorCode::Refused, issues.join("; "))
        })?;
        let network = rendered
            .packages
            .get(&ManagedPackage::Network)
            .ok_or_else(|| failed(JobErrorCode::Refused, "Render wrote no network package"))?;
        let current = self
            .current
            .get("network")
            .cloned()
            .unwrap_or_else(|| uniwrt_uci::Package::new("network"));
        Ok(patch(&current, network, &self.facts.board.layout).to_file())
    }

    /// The Stage and Apply steps of the first trial.
    pub(crate) async fn try_file(
        &mut self,
        id: &str,
        file: String,
        progress: &mut Progress,
    ) -> Result<(), JobError> {
        progress.start(StepName::Stage).await;
        let txn = TxnId::new(id).map_err(|err| internal("naming the transaction", &err))?;
        self.stage(&txn, file)
            .await
            .map_err(|why| failed(JobErrorCode::NodeFailed, why))?;
        progress
            .node(self.node.id, JobNodeState::Staged, "Staged and checked")
            .await;
        progress.done(Some("Staged and checked".to_owned())).await;
        progress.start(StepName::Apply).await;
        self.arm(txn)
            .await
            .map_err(|why| failed(JobErrorCode::NodeFailed, why))?;
        let timer = self.shared.timing.timer_uplink.as_secs();
        let detail = format!("Applied; it rolls back by itself in {timer} s");
        progress
            .node(self.node.id, JobNodeState::Applied, &detail)
            .await;
        progress.done(Some(detail)).await;
        Ok(())
    }

    /// Stages and applies `file` in one go (the 6rd trial's).
    pub(crate) async fn apply(&mut self, id: &str, file: String) -> Result<(), String> {
        let txn = TxnId::new(id).map_err(|err| err.to_string())?;
        self.stage(&txn, file).await?;
        self.arm(txn).await
    }

    async fn stage(&self, txn: &TxnId, file: String) -> Result<(), String> {
        let staged = async {
            self.tool(&txn::stage(txn, [])?).await?;
            self.tool(&txn::put(txn, "network", file)?).await?;
            self.tool(&txn::check(txn)).await
        }
        .await;
        staged
            .map(|_| ())
            .map_err(|err| format!("It refused the WAN: {}", chain(&err)))
    }

    async fn arm(&mut self, txn: TxnId) -> Result<(), String> {
        let command = txn::apply(&txn, self.shared.timing.timer_uplink);
        self.open = Some(txn);
        match self.tool(&command).await {
            Err(err @ uniwrt_node::Error::Txn { .. }) => {
                self.open = None;
                Err(format!("It could not apply the WAN: {}", chain(&err)))
            }
            // Applied, or the answer is lost (not necessarily the apply): the checks tell.
            _ => Ok(()),
        }
    }

    /// Runs a command of the transaction tool.
    async fn tool<T: serde::de::DeserializeOwned>(
        &self,
        command: &txn::Command<T>,
    ) -> uniwrt_node::Result<T> {
        let output = self.remote.exec(command.request.clone()).await?;
        command.parse(output)
    }

    /// Rolls the open transaction back, and says how it went.
    pub(crate) async fn roll_back(&mut self) -> String {
        let Some(txn) = self.open.take() else {
            return "Nothing was applied".to_owned();
        };
        match self.tool(&txn::rollback(&txn)).await {
            Ok(_) => "The gateway holds its own WAN again".to_owned(),
            Err(uniwrt_node::Error::Txn { code, .. }) if code == txn::code::EXPIRED => {
                "It had rolled back by itself already".to_owned()
            }
            Err(err) => format!(
                "The rollback failed ({}); it rolls back by itself within {} s",
                chain(&err),
                self.shared.timing.timer_uplink.as_secs()
            ),
        }
    }

    pub(crate) async fn exec(&self, request: ExecRequest) -> Option<ExecOutput> {
        self.remote.exec(request).await.ok()
    }

    /// Reads the facts until `read` finds something or `within` passes; what it found, and the
    /// last facts read.
    pub(crate) async fn wait<T>(
        &self,
        within: Duration,
        mut read: impl FnMut(&NodeFacts) -> Option<T>,
    ) -> (Option<T>, Option<NodeFacts>) {
        let deadline = Instant::now() + within;
        let mut last = None;
        loop {
            if let Ok(facts) = survey::collect(self.shared, self.remote.as_ref(), &SETS, &[]).await
            {
                if let Some(found) = read(&facts) {
                    return (Some(found), Some(facts));
                }
                last = Some(facts);
            }
            if Instant::now() >= deadline {
                return (None, last);
            }
            sleep(self.shared.timing.poll).await;
        }
    }
}
