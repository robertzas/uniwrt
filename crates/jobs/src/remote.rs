//! How jobs reach nodes: [`Dial`] probes and logs in, and a [`Remote`] is one login. Over SSH in
//! production ([`SshDial`]); tests script them.

use std::{pin::Pin, time::Duration};

use tokio::sync::{OnceCell, broadcast};
use uniwrt_node::{
    Agent, AgentLaunch, Credential, ExecOutput, ExecRequest, HELLO_TIMEOUT, HostKey, Result,
    Session, SshConfig, Target,
    proto::ops::{Collect, Collected, Op as _},
};

/// A boxed future, so that the traits stay dyn-compatible.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How long the agent may take to collect every set once.
const COLLECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Reaches nodes.
pub trait Dial: Send + Sync + 'static {
    /// Reads the node's host key without logging in (trust on first use).
    fn probe<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<HostKey>>;

    /// Logs in, requiring the node to present `host_key`.
    fn connect<'a>(
        &'a self,
        target: &'a Target,
        host_key: &'a HostKey,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>>;
}

/// One login on a node.
pub trait Remote: Send + Sync {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>>;

    /// Every collect set, from the agent, which starts on first use (installed into `/tmp`).
    fn collect(&self) -> BoxFuture<'_, Result<Collected>>;

    /// Logs out; the agent, if started, exits with it.
    fn close(&self) -> BoxFuture<'_, ()>;
}

/// [`Dial`] over SSH.
#[derive(Debug, Clone, Default)]
pub struct SshDial {
    pub config: SshConfig,
}

impl Dial for SshDial {
    fn probe<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<HostKey>> {
        Box::pin(uniwrt_node::probe(target, None, &self.config))
    }

    fn connect<'a>(
        &'a self,
        target: &'a Target,
        host_key: &'a HostKey,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Box<dyn Remote>>> {
        Box::pin(async move {
            let session =
                Session::connect(target, None, host_key, credential, &self.config).await?;
            Ok(Box::new(SshRemote {
                session,
                agent: OnceCell::new(),
            }) as Box<dyn Remote>)
        })
    }
}

struct SshRemote {
    session: Session,
    agent: OnceCell<Agent>,
}

impl Remote for SshRemote {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>> {
        Box::pin(async move { self.session.exec(&request).await })
    }

    fn collect(&self) -> BoxFuture<'_, Result<Collected>> {
        Box::pin(async move {
            let agent = self
                .agent
                .get_or_try_init(|| async {
                    // Nothing listens for pushes: the agent only answers here.
                    let events = broadcast::channel(1).0;
                    AgentLaunch::Bundle
                        .start(&self.session, &events, HELLO_TIMEOUT)
                        .await
                })
                .await?;
            let args = serde_json::value::to_raw_value(&Collect::all())
                .map_err(|err| uniwrt_node::Error::Protocol(err.to_string()))?;
            let answer = agent.call(Collect::NAME, &args, COLLECT_TIMEOUT).await?;
            serde_json::from_str(answer.get()).map_err(|err| {
                uniwrt_node::Error::Protocol(format!("unexpected answer to collect: {err}"))
            })
        })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(self.session.close())
    }
}
