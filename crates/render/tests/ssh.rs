//! SSH hardening (features/security.md, Node hardening; D-150): the node's own `dropbear` file
//! with passwords off and the server bound to the management network's interface, the gateway
//! bound only while this controller logs in from that network, and the running servers checked
//! against what the file asks.

// Tests and their fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

mod support;

use std::{collections::BTreeMap, net::IpAddr};

use uniwrt_model::{
    facts::SshServerFacts,
    node::NodeRole,
    site::{
        SiteConfig,
        network::{Ipv6Mode, Purpose},
        node::{ManagedPackage, SshHardening},
    },
};
use uniwrt_render::{Issue, Rendered, SshServers, Target, render};
use uniwrt_uci::Package;

use support::*;

/// OpenWrt's stock packages, with 25.12's `dropbear` and a banner the node chose.
fn current() -> BTreeMap<String, Package> {
    let mut current = stock();
    let text = include_str!("../../uci/fixtures/openwrt/25.12/dropbear");
    let mut dropbear = uniwrt_uci::parse("dropbear", text).unwrap();
    dropbear.sections[0].set("BannerFile", "/etc/banner");
    current.insert("dropbear".to_owned(), dropbear);
    current
}

/// The home site with both nodes hardened as `ssh` says.
fn hardened(ssh: SshHardening) -> SiteConfig {
    let mut site = home_site();
    for node in &mut site.nodes {
        node.managed.insert(ManagedPackage::Dropbear);
        node.ssh = ssh;
    }
    site
}

fn on(key_only: bool, management_only: bool) -> SshHardening {
    SshHardening {
        key_only,
        management_only,
    }
}

fn run_at(
    site: &SiteConfig,
    node: uniwrt_core::NodeId,
    role: NodeRole,
    controller: Option<IpAddr>,
    current: &BTreeMap<String, Package>,
) -> Result<Rendered, Vec<Issue>> {
    let facts = gateway_facts();
    render(
        site,
        &secrets(),
        Target {
            node,
            role,
            facts: &facts,
            current,
            controller,
            backhaul: &[],
        },
    )
}

fn dropbear(result: Result<Rendered, Vec<Issue>>) -> String {
    let rendered = result.unwrap_or_else(|issues| panic!("{issues:?}"));
    rendered.packages[&ManagedPackage::Dropbear].to_file()
}

#[test]
fn key_only_turns_passwords_off_and_keeps_the_rest_of_the_node_s_file() {
    let site = hardened(on(true, false));
    for (node, role) in [(GATEWAY, NodeRole::Gateway), (AP, NodeRole::Ap)] {
        assert_eq!(
            dropbear(run_at(&site, node, role, None, &current())),
            "\nconfig dropbear 'main'\n\toption enable '1'\n\toption PasswordAuth 'off'\n\
             \toption RootPasswordAuth 'off'\n\toption Port '22'\n\
             \toption BannerFile '/etc/banner'\n\n",
            "{role:?}"
        );
    }
}

#[test]
fn the_management_network_binds_the_server_to_its_interface() {
    // Without a management network, the default one is it.
    let site = hardened(on(false, true));
    let text = dropbear(run_at(&site, AP, NodeRole::Ap, None, &current()));
    assert!(text.contains("\toption PasswordAuth 'on'\n"), "{text}");
    assert!(text.contains("\toption DirectInterface 'lan'\n"), "{text}");

    let mut site = hardened(on(true, true));
    site.networks
        .push(net("mgmt", "Management", Purpose::Mgmt, 99, Ipv6Mode::Off));
    let text = dropbear(run_at(&site, AP, NodeRole::Ap, None, &current()));
    assert!(text.contains("\toption DirectInterface 'mgmt'\n"), "{text}");
    let controller = "10.0.99.20".parse().ok();
    let text = dropbear(run_at(
        &site,
        GATEWAY,
        NodeRole::Gateway,
        controller,
        &current(),
    ));
    assert!(text.contains("\toption DirectInterface 'mgmt'\n"), "{text}");
}

#[test]
fn the_gateway_isn_t_bound_where_this_controller_logs_in_from_elsewhere() {
    let mut site = hardened(on(true, true));
    site.networks
        .push(net("mgmt", "Management", Purpose::Mgmt, 99, Ipv6Mode::Off));
    for from in ["10.0.0.20", "fd12::20"] {
        let issues = run_at(
            &site,
            GATEWAY,
            NodeRole::Gateway,
            from.parse().ok(),
            &current(),
        )
        .unwrap_err();
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].path, "nodes/1/ssh/management_only");
        assert!(issues[0].message.contains(from), "{}", issues[0].message);
        assert!(
            issues[0].message.contains("10.0.99.0/24"),
            "{}",
            issues[0].message
        );
    }
    // An AP is reached on its only address whatever the route.
    let text = dropbear(run_at(
        &site,
        AP,
        NodeRole::Ap,
        "10.0.0.20".parse().ok(),
        &current(),
    ));
    assert!(text.contains("DirectInterface 'mgmt'"), "{text}");
}

#[test]
fn turning_it_off_undoes_it_and_a_node_without_the_file_gets_openwrt_s() {
    let site = hardened(on(true, true));
    let applied = dropbear(run_at(&site, AP, NodeRole::Ap, None, &current()));
    let mut held = current();
    held.insert(
        "dropbear".to_owned(),
        uniwrt_uci::parse("dropbear", &applied).unwrap(),
    );
    let undone = dropbear(run_at(
        &hardened(on(false, false)),
        AP,
        NodeRole::Ap,
        None,
        &held,
    ));
    assert_eq!(
        undone,
        "\nconfig dropbear 'main'\n\toption enable '1'\n\toption PasswordAuth 'on'\n\
         \toption RootPasswordAuth 'on'\n\toption Port '22'\n\
         \toption BannerFile '/etc/banner'\n\n"
    );
    // Without a file and with nothing on, the package isn't render's to write.
    let none = run_at(
        &hardened(on(false, false)),
        AP,
        NodeRole::Ap,
        None,
        &stock(),
    )
    .unwrap();
    assert!(!none.packages.contains_key(&ManagedPackage::Dropbear));
    assert_eq!(
        dropbear(run_at(&site, AP, NodeRole::Ap, None, &stock())),
        "\nconfig dropbear 'main'\n\toption enable '1'\n\toption PasswordAuth 'off'\n\
         \toption RootPasswordAuth 'off'\n\toption Port '22'\n\toption DirectInterface 'lan'\n\n"
    );
}

fn server(password: bool, device: Option<&str>) -> SshServerFacts {
    SshServerFacts {
        running: true,
        password,
        root_password: password,
        device: device.map(str::to_owned),
        listen: vec!["22".to_owned()],
    }
}

#[test]
fn the_running_servers_are_checked_against_the_file() {
    let file = dropbear(run_at(
        &hardened(on(true, true)),
        AP,
        NodeRole::Ap,
        None,
        &current(),
    ));
    let want = SshServers::of(&uniwrt_uci::parse("dropbear", &file).unwrap()).unwrap();
    assert_eq!(
        want,
        SshServers {
            password: false,
            root_password: false,
            bound: true
        }
    );
    assert_eq!(want.check(Some(&[server(false, Some("br-lan"))])), Ok(()));
    let stale = want.check(Some(&[server(true, None)])).unwrap_err();
    assert!(stale.contains("password logins on"), "{stale}");
    let open = want.check(Some(&[server(false, None)])).unwrap_err();
    assert!(open.contains("every network"), "{open}");
    let mut stopped = server(false, Some("br-lan"));
    stopped.running = false;
    assert_eq!(
        want.check(Some(&[stopped])),
        Err("No SSH server runs".to_owned())
    );
    assert!(want.check(None).is_err());
    // A disabled section asks nothing.
    let off = uniwrt_uci::parse("dropbear", "config dropbear\n\toption enable '0'\n").unwrap();
    assert_eq!(SshServers::of(&off), None);
}
