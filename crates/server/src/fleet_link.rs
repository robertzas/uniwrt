//! [`FleetLink`]: the one link type the engine runs on, so the API has one engine type
//! whether the nodes are real (SSH) or simulated (demo mode).

use std::time::Duration;

use bytes::Bytes;
use serde_json::value::RawValue;
use tokio::sync::{broadcast, watch};
use uniwrt_node::{
    AgentEvent, ExecOutput, ExecRequest, ExecStream, LinkState, NodeLink, Result, SshLink,
};
use uniwrt_sim::SimLink;

pub(crate) enum FleetLink {
    Ssh(SshLink),
    Sim(SimLink),
}

impl FleetLink {
    /// Disconnects a real node; a simulated one has nothing to close.
    pub(crate) async fn shutdown(&self) {
        if let Self::Ssh(link) = self {
            link.shutdown().await;
        }
    }
}

impl NodeLink for FleetLink {
    async fn call(&self, op: &str, args: &RawValue, timeout: Duration) -> Result<Box<RawValue>> {
        match self {
            Self::Ssh(link) => link.call(op, args, timeout).await,
            Self::Sim(link) => link.call(op, args, timeout).await,
        }
    }

    async fn exec(&self, request: ExecRequest) -> Result<ExecOutput> {
        match self {
            Self::Ssh(link) => link.exec(request).await,
            Self::Sim(link) => link.exec(request).await,
        }
    }

    async fn stream(&self, argv: Vec<String>) -> Result<ExecStream> {
        match self {
            Self::Ssh(link) => link.stream(argv).await,
            Self::Sim(link) => link.stream(argv).await,
        }
    }

    async fn upload(&self, path: &str, data: Bytes, mode: u32) -> Result<()> {
        match self {
            Self::Ssh(link) => link.upload(path, data, mode).await,
            Self::Sim(link) => link.upload(path, data, mode).await,
        }
    }

    fn events(&self) -> broadcast::Receiver<AgentEvent> {
        match self {
            Self::Ssh(link) => link.events(),
            Self::Sim(link) => link.events(),
        }
    }

    fn state(&self) -> watch::Receiver<LinkState> {
        match self {
            Self::Ssh(link) => link.state(),
            Self::Sim(link) => link.state(),
        }
    }
}
