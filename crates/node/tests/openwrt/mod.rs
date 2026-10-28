//! Shared by the tests against OpenWrt rootfs containers (`dropbear.rs`, `agent.rs`): adopting
//! the node at `UNIWRT_NODE=<host:port>` with the root password in `UNIWRT_NODE_PASSWORD`.

#![allow(dead_code, unreachable_pub, clippy::unwrap_used)]

use std::env;

use uniwrt_node::{
    ControllerKey, Credential, ExecRequest, HostKey, Session, SshConfig, Target, probe,
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
    let (host, port) = address.rsplit_once(':').unwrap();
    let target = Target::new(host, port.parse().unwrap());
    let config = SshConfig::default();
    let host_key = probe(&target, None, &config).await.unwrap();
    assert_eq!(host_key.algorithm(), "ssh-ed25519");

    let password = env::var("UNIWRT_NODE_PASSWORD").expect("UNIWRT_NODE_PASSWORD");
    let password = Credential::Password(Zeroizing::new(password));
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

pub async fn run(session: &Session, argv: &[&str]) -> String {
    let output = session
        .exec(&ExecRequest::new(argv.iter().copied()))
        .await
        .unwrap();
    let output = output.check(argv[0]).unwrap();
    String::from_utf8(output.stdout).unwrap()
}
