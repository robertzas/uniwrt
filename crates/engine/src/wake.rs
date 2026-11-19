//! Wake-on-LAN from the gateway (features/network.md, Other services; D-137): the magic packet
//! goes out on the gateway's devices that reach the client ([`uniwrt_node::wol::devices`])
//! through `etherwake`. A gateway without it gets it from the feeds (the index refreshed, then
//! the package installed) when the caller may change it (`may_install`: it is managed), else
//! the wake is refused with [`WakeError::NoSender`].

use std::{net::IpAddr, time::Duration};

use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::status::NodeState;
use uniwrt_node::{ExecOutput, ExecRequest, ExitStatus, pkg, wol};

use crate::Fleet;

/// How long sending one packet may take.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long refreshing the index or installing may take: slow mirrors on slow routers.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);

/// Where a wake went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Woken {
    pub node: NodeId,
    /// The gateway's devices the packet went out on.
    pub devices: Vec<String>,
    /// Whether `etherwake` was installed for it.
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WakeError {
    #[error("the gateway is offline")]
    Offline,
    #[error("the gateway serves no network")]
    NoNetwork,
    /// The gateway lacks `etherwake`, and may not be changed.
    #[error("the gateway has no etherwake")]
    NoSender,
    #[error("{0}")]
    Failed(String),
}

/// Wakes `mac`, whose last addresses were `ips`, from `gateway`.
pub async fn wake(
    fleet: &dyn Fleet,
    gateway: NodeId,
    may_install: bool,
    mac: MacAddr,
    ips: &[IpAddr],
) -> Result<Woken, WakeError> {
    let view = fleet.node(gateway).ok_or(WakeError::Offline)?;
    let (NodeState::Online | NodeState::Degraded, Some(facts)) = (view.status.state, view.facts)
    else {
        return Err(WakeError::Offline);
    };
    let devices = wol::devices(&facts, mac, ips);
    if devices.is_empty() {
        return Err(WakeError::NoNetwork);
    }
    let mut installed = false;
    for device in &devices {
        let request = || ExecRequest::new(wol::command(device, mac)).timeout(SEND_TIMEOUT);
        let mut output = run(fleet, gateway, request()).await?;
        if output.status == ExitStatus::Code(wol::NOT_FOUND) && !installed {
            if !may_install {
                return Err(WakeError::NoSender);
            }
            let manager = facts.capabilities.package_manager.ok_or_else(|| {
                WakeError::Failed("The gateway has no package manager".to_owned())
            })?;
            for argv in [pkg::update(manager), pkg::install(manager, &[wol::PACKAGE])] {
                let what = argv.join(" ");
                let request = ExecRequest::new(argv).timeout(INSTALL_TIMEOUT);
                let done = run(fleet, gateway, request).await?;
                check(&what, done)?;
            }
            installed = true;
            output = run(fleet, gateway, request()).await?;
        }
        check("etherwake", output)?;
    }
    Ok(Woken {
        node: gateway,
        devices,
        installed,
    })
}

async fn run(
    fleet: &dyn Fleet,
    node: NodeId,
    request: ExecRequest,
) -> Result<ExecOutput, WakeError> {
    let what = request.argv.join(" ");
    fleet
        .exec(node, request)
        .await
        .map_err(|err| WakeError::Failed(format!("{what}: {err}")))
}

fn check(what: &str, output: ExecOutput) -> Result<(), WakeError> {
    if output.status.success() {
        return Ok(());
    }
    let said = if output.stderr.is_empty() {
        output.stdout
    } else {
        output.stderr
    };
    let said = String::from_utf8_lossy(&said);
    let last = said
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Err(WakeError::Failed(format!(
        "{what} failed ({:?}): {}",
        output.status,
        last.trim()
    )))
}
