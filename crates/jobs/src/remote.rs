//! How jobs reach nodes: [`Dial`] reads banners, probes and logs in, and a [`Remote`] is one login. Over SSH in
//! production ([`SshDial`]); tests script them.

use std::{pin::Pin, time::Duration};

use serde_json::value::RawValue;

use tokio::sync::{OnceCell, broadcast};
use uniwrt_node::{
    Agent, AgentLaunch, Credential, ExecOutput, ExecRequest, HELLO_TIMEOUT, HostKey, Result,
    Session, SshConfig, Target,
    proto::ops::{Collect, Collected, Op, Set},
};

/// A boxed future, so that the traits stay dyn-compatible.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How long an SSH server may take to send its banner (it does at once).
const BANNER_TIMEOUT: Duration = Duration::from_secs(3);

/// How long the agent may take to collect every set once.
const COLLECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Reaches nodes.
pub trait Dial: Send + Sync + 'static {
    /// Reads the SSH server's version line on port `target.port`, without a handshake; `None`
    /// when something else listens there (discovery).
    fn banner<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<Option<String>>>;

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

    /// The collect `sets`, from the agent, which starts on first use (installed into `/tmp`).
    fn collect<'a>(&'a self, sets: &'a [Set]) -> BoxFuture<'a, Result<Collected>>;

    /// The agent's answer to `op` (one of `uniwrt_node::proto::ops`), within `timeout`; the
    /// agent starts as for [`Remote::collect`]. Logins that can't reach an agent answer
    /// [`uniwrt_node::Error::Protocol`].
    fn call<'a>(
        &'a self,
        op: &'static str,
        args: &'a RawValue,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Box<RawValue>>> {
        let _ = (args, timeout);
        Box::pin(async move {
            Err(uniwrt_node::Error::Protocol(format!(
                "this login can't call the agent's {op}"
            )))
        })
    }

    /// Logs out; the agent, if started, exits with it.
    fn close(&self) -> BoxFuture<'_, ()>;
}

/// `op`'s answer over `remote`, within `timeout`.
pub(crate) async fn request<O: Op>(
    remote: &dyn Remote,
    op: &O,
    timeout: Duration,
) -> Result<O::Output> {
    let args = serde_json::value::to_raw_value(op)
        .map_err(|err| uniwrt_node::Error::Protocol(err.to_string()))?;
    let answer = remote.call(O::NAME, &args, timeout).await?;
    serde_json::from_str(answer.get()).map_err(|err| {
        uniwrt_node::Error::Protocol(format!("unexpected answer to {}: {err}", O::NAME))
    })
}

/// [`Dial`] over SSH.
#[derive(Debug, Clone, Default)]
pub struct SshDial {
    pub config: SshConfig,
}

impl Dial for SshDial {
    fn banner<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<Option<String>>> {
        let timeout = BANNER_TIMEOUT.min(self.config.connect_timeout);
        Box::pin(uniwrt_node::banner(target, timeout))
    }

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

impl SshRemote {
    /// The agent, started on first use.
    async fn agent(&self) -> Result<&Agent> {
        self.agent
            .get_or_try_init(|| async {
                // Nothing listens for pushes: the agent only answers here.
                let events = broadcast::channel(1).0;
                AgentLaunch::Bundle
                    .start(&self.session, &events, HELLO_TIMEOUT)
                    .await
            })
            .await
    }
}

impl Remote for SshRemote {
    fn exec(&self, request: ExecRequest) -> BoxFuture<'_, Result<ExecOutput>> {
        Box::pin(async move { self.session.exec(&request).await })
    }

    fn collect<'a>(&'a self, sets: &'a [Set]) -> BoxFuture<'a, Result<Collected>> {
        Box::pin(async move {
            let args = serde_json::value::to_raw_value(&Collect {
                sets: sets.to_vec(),
            })
            .map_err(|err| uniwrt_node::Error::Protocol(err.to_string()))?;
            let answer = self
                .agent()
                .await?
                .call(Collect::NAME, &args, COLLECT_TIMEOUT)
                .await?;
            serde_json::from_str(answer.get()).map_err(|err| {
                uniwrt_node::Error::Protocol(format!("unexpected answer to collect: {err}"))
            })
        })
    }

    fn call<'a>(
        &'a self,
        op: &'static str,
        args: &'a RawValue,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<Box<RawValue>>> {
        Box::pin(async move { self.agent().await?.call(op, args, timeout).await })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(self.session.close())
    }
}
