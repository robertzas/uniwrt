//! A look on the adoption port (features/adoption.md, Path C; D-106): whether a factory-default
//! OpenWrt device answers at 192.168.1.1 on the gateway's adoption port, and if so, its entry in
//! the pending inbox. The server asks for a look when the gateway's agent reports a link on the
//! port or its bridge, and when the gateway comes online; it looks again while nothing answers
//! yet, as a device takes a minute or two to boot.
//!
//! A look logs in to the gateway (the controller's key) and checks that the segment is up at
//! the site's address (`uw_adopt`: only then is 192.168.1.1 a device on the port, never the
//! gateway's own LAN). Through that login (`direct-tcpip`) it reads the device's banner
//! (dropbear's), its host key, then logs in without a password, which only factory defaults
//! accept, to read the board and the identity MAC as a ZTP hello reports them. A device whose
//! root has a password is listed locked, by the MAC the gateway's ARP table has for it. Nothing
//! changes on either.

use std::net::IpAddr;

use tracing::{debug, info};
use uniwrt_core::{MacAddr, NodeId};
use uniwrt_model::{
    node::{Node, NodeRole},
    site::adoption::AdoptionPort,
    ztp::{PendingDevice, PendingSource, ZtpInbox},
};
use uniwrt_node::{
    Credential, ExecRequest, HostKey, Target,
    adoption_port::{self as port, Board},
};
use uniwrt_store::{NodeRepo as _, RevisionRepo as _, ZtpRepo as _};
use zeroize::Zeroizing;

use crate::{Dial, Error, Jobs, Remote, Result, manage, runner::Shared};

const SSH_PORT: u16 = 22;

/// What a look found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortLook {
    /// Nothing to look at: the saved site has no adoption port, the node is not its gateway,
    /// or the gateway's segment is not up.
    NotThePort,
    /// The gateway can't be reached now.
    Unreachable,
    /// The segment is up, but no OpenWrt answers at 192.168.1.1 (yet).
    Nothing,
    /// An adopted node answered there.
    Adopted(NodeId),
    /// A device answered: its entry in the inbox, as stored.
    Found(Box<PendingDevice>),
}

/// What the device itself showed.
struct Reached {
    host_key: HostKey,
    /// Root has a password: nothing more was read.
    locked: bool,
    board: Board,
    mac: Option<MacAddr>,
}

impl Jobs {
    /// Looks once on the adoption port of `gateway`.
    pub async fn look_on_adoption_port(&self, gateway: NodeId) -> Result<PortLook> {
        let shared = &self.shared;
        if shared.dial.is_none() {
            return Err(Error::Unavailable);
        }
        let Some(site_port) = shared
            .store
            .head_revision()
            .await?
            .and_then(|head| head.config.adoption_port)
        else {
            return Ok(PortLook::NotThePort);
        };
        let nodes = shared.store.nodes().await?;
        let Some(node) = nodes
            .iter()
            .find(|node| node.id == gateway && node.role == NodeRole::Gateway)
        else {
            return Ok(PortLook::NotThePort);
        };
        let remote = match manage::login(shared, node).await {
            Ok(remote) => remote,
            Err(err) => {
                debug!(gateway = %node.id, message = %err.message, "no look on the adoption port");
                return Ok(PortLook::Unreachable);
            }
        };
        let look = look(shared, remote.as_ref(), node, &site_port, &nodes).await;
        remote.close().await;
        look
    }
}

async fn look(
    shared: &Shared,
    gateway: &dyn Remote,
    node: &Node,
    site_port: &AdoptionPort,
    nodes: &[Node],
) -> Result<PortLook> {
    let up = text(gateway, port::segment_status())
        .await
        .is_some_and(|status| port::segment_up(&status, site_port.address));
    let Some(tunnel) = gateway.dial().filter(|_| up) else {
        return Ok(PortLook::NotThePort);
    };
    let target = Target::new(AdoptionPort::FACTORY.to_string(), SSH_PORT);
    let Some(reached) = reach(tunnel.as_ref(), &target).await else {
        return Ok(PortLook::Nothing);
    };
    let mac = match reached.mac {
        Some(mac) => Some(mac),
        None => text(gateway, port::arp_table())
            .await
            .and_then(|arp| port::neighbor(&arp, AdoptionPort::FACTORY, port::BRIDGE)),
    };
    let Some(mac) = mac else {
        debug!(gateway = %node.id, "the device on the adoption port has no MAC address to show");
        return Ok(PortLook::Nothing);
    };
    if let Some(adopted) = nodes.iter().find(|node| node.mac == mac) {
        return Ok(PortLook::Adopted(adopted.id));
    }
    let now = shared.clock.now();
    let board = reached.board;
    let device = PendingDevice {
        mac,
        board_name: board.board_name,
        model: board.model,
        hostname: board.hostname,
        release: board.release.version,
        role: None,
        address: IpAddr::V4(AdoptionPort::FACTORY),
        source: PendingSource::Port {
            gateway: node.id,
            port: site_port.port.clone(),
        },
        locked: reached.locked,
        host_key: reached.host_key.to_openssh().to_owned(),
        fingerprint: reached.host_key.fingerprint().to_owned(),
        first_seen: now,
        last_seen: now,
        job: None,
    };
    let stored = shared
        .store
        .ztp_hello(device, ZtpInbox::MAX_PENDING)
        .await?;
    info!(%mac, gateway = %node.id, locked = stored.locked, model = %stored.model,
        "a device answers on the adoption port");
    Ok(PortLook::Found(Box::new(stored)))
}

/// The device at `target`, if OpenWrt's dropbear answers there.
async fn reach(dial: &dyn Dial, target: &Target) -> Option<Reached> {
    let banner = dial.banner(target).await.ok().flatten()?;
    if !banner.to_ascii_lowercase().contains("dropbear") {
        return None;
    }
    let host_key = dial.probe(target).await.ok()?;
    let blank = Credential::Password(Zeroizing::new(String::new()));
    let device = match dial.connect(target, &host_key, &blank).await {
        Ok(device) => device,
        Err(uniwrt_node::Error::AuthRejected { .. }) => {
            return Some(Reached {
                host_key,
                locked: true,
                board: Board::default(),
                mac: None,
            });
        }
        Err(err) => {
            debug!(%err, "no login on the adoption port's device");
            return None;
        }
    };
    let board = text(device.as_ref(), port::board())
        .await
        .and_then(|board| Board::parse(&board))
        .unwrap_or_default();
    let mac = text(device.as_ref(), port::identity())
        .await
        .and_then(|output| port::identity_mac(&output));
    device.close().await;
    Some(Reached {
        host_key,
        locked: false,
        board,
        mac,
    })
}

/// A command's output, if it ran and succeeded.
async fn text(remote: &dyn Remote, request: ExecRequest) -> Option<String> {
    let what = request.argv.first().cloned().unwrap_or_default();
    let output = remote.exec(request).await.ok()?.check(&what).ok()?;
    String::from_utf8(output.stdout).ok()
}
