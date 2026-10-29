//! Shared by the tests against OpenWrt rootfs containers (`dropbear.rs`, `agent.rs`, `wifi.rs`):
//! adopting a node (by default the one at `UNIWRT_NODE=<host:port>`, with the root password in
//! `UNIWRT_NODE_PASSWORD`), linking to its agent, and waiting for its pushes.

#![allow(dead_code, unreachable_pub, clippy::unwrap_used)]

use std::{env, sync::Arc, time::Duration};

use tokio::sync::broadcast;
use uniwrt_core::SystemClock;
use uniwrt_node::{
    AgentEvent, AgentLaunch, AgentState, ControllerKey, Credential, ExecRequest, Hello, HostKey,
    LinkConfig, LinkState, NodeLink as _, Session, SshConfig, SshLink, Target,
    events::{self, Push},
    probe,
};
use zeroize::Zeroizing;

pub struct Node {
    pub target: Target,
    pub host_key: HostKey,
    pub key: ControllerKey,
}

/// Adopts the container as the adoption wizard will (M1.7): read the host key (the user would
/// confirm it), log in with the root password, install the controller key, and log in with it.
pub async fn adopt() -> (Session, Node) {
    let address = env::var("UNIWRT_NODE").expect("UNIWRT_NODE=<host:port>");
    let password = env::var("UNIWRT_NODE_PASSWORD").expect("UNIWRT_NODE_PASSWORD");
    adopt_at(&address, &password).await
}

/// [`adopt`] for the node at `address` (`<host:port>`) with this root password.
pub async fn adopt_at(address: &str, password: &str) -> (Session, Node) {
    let (host, port) = address.rsplit_once(':').unwrap();
    let target = Target::new(host, port.parse().unwrap());
    let config = SshConfig::default();
    let host_key = probe(&target, None, &config).await.unwrap();
    assert_eq!(host_key.algorithm(), "ssh-ed25519");

    let password = Credential::Password(Zeroizing::new(password.to_owned()));
    let session = Session::connect(&target, None, &host_key, &password, &config)
        .await
        .unwrap();
    let key = ControllerKey::generate().unwrap();
    let install = r#"umask 077 && printf '%s\n' "$1" >>/etc/dropbear/authorized_keys"#;
    let line = key.authorized_key().unwrap();
    session
        .exec(&ExecRequest::new(["sh", "-c", install, "sh", &line]))
        .await
        .unwrap()
        .check("installing the key")
        .unwrap();

    let credential = Credential::Key(key.clone());
    let session = Session::connect(&target, None, &host_key, &credential, &config)
        .await
        .unwrap();
    (
        session,
        Node {
            target,
            host_key,
            key,
        },
    )
}

/// A link to the adopted node that runs `launch`, once its agent has said hello.
pub async fn link(node: &Node, launch: AgentLaunch) -> (SshLink, Hello) {
    let credential = Credential::Key(node.key.clone());
    let mut config = LinkConfig::new(node.target.clone(), node.host_key.clone(), credential);
    config.agent = Some(launch);
    let link = SshLink::spawn(config, Arc::new(SystemClock));
    let mut state = link.state();
    let up = tokio::time::timeout(Duration::from_secs(20), async {
        state
            .wait_for(|state| {
                matches!(
                    state,
                    LinkState::Up {
                        agent: AgentState::Running(_) | AgentState::Failed { .. },
                        ..
                    }
                )
            })
            .await
            .unwrap()
            .clone()
    })
    .await
    .unwrap();
    let LinkState::Up {
        agent: AgentState::Running(hello),
        ..
    } = up
    else {
        panic!("{up:?}");
    };
    (link, hello)
}

pub async fn run(session: &Session, argv: &[&str]) -> String {
    let output = session
        .exec(&ExecRequest::new(argv.iter().copied()))
        .await
        .unwrap();
    let output = output.check(argv[0]).unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// Waits up to `limit` for the first push that `want` picks, skipping the others.
pub async fn next_push<T>(
    pushes: &mut broadcast::Receiver<AgentEvent>,
    limit: Duration,
    mut want: impl FnMut(Push) -> Option<T>,
) -> T {
    tokio::time::timeout(limit, async {
        loop {
            let event = pushes.recv().await.unwrap();
            if let Some(found) = events::parse(&event).unwrap().and_then(&mut want) {
                return found;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "no such push within {limit:?}: {}",
            std::any::type_name::<T>()
        )
    })
}
