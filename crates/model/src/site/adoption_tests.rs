//! The adoption port (schema v10, D-105): its migration and validation.

use std::{collections::BTreeSet, net::Ipv4Addr};

use serde_json::Value;
use uniwrt_core::NodeId;

use super::{
    SiteConfig,
    adoption::AdoptionPort,
    gateway::{BackupLink, BackupProto, Failover},
    migrate::upgrade,
    validate::{Severity, validate},
};

fn fixture(version: u32) -> Value {
    let text = match version {
        9 => include_str!("../../fixtures/site/v9.json"),
        _ => include_str!("../../fixtures/site/v10.json"),
    };
    serde_json::from_str(text).unwrap()
}

/// The (path, severity) of the issues of the v10 fixture after `edit`.
fn issues_after(edit: impl FnOnce(&mut SiteConfig)) -> Vec<(String, Severity)> {
    let mut config = upgrade(fixture(10)).unwrap();
    edit(&mut config);
    validate(&config, &BTreeSet::from([NodeId::new(1), NodeId::new(2)]))
        .into_iter()
        .map(|issue| (issue.path, issue.severity))
        .collect()
}

fn error(path: &str) -> Vec<(String, Severity)> {
    vec![(path.to_owned(), Severity::Error)]
}

#[test]
fn v9_documents_have_no_adoption_port() {
    assert_eq!(upgrade(fixture(9)).unwrap().adoption_port, None);
    assert_eq!(
        upgrade(fixture(10)).unwrap().adoption_port,
        Some(AdoptionPort::new("lan4"))
    );
}

#[test]
fn the_gateways_address_on_the_port_is_a_host_of_the_factory_subnet() {
    assert_eq!(
        AdoptionPort::new("lan4").interface().to_string(),
        "192.168.1.250/24"
    );
    for good in [[192, 168, 1, 2], [192, 168, 1, 254]] {
        let at = |c: &mut SiteConfig| {
            c.adoption_port.as_mut().unwrap().address = Ipv4Addr::from(good);
        };
        assert_eq!(issues_after(at), [], "{good:?}");
    }
    for bad in [
        [192, 168, 1, 0],
        [192, 168, 1, 1],
        [192, 168, 1, 255],
        [192, 168, 2, 250],
        [10, 0, 0, 250],
    ] {
        let at = |c: &mut SiteConfig| {
            c.adoption_port.as_mut().unwrap().address = Ipv4Addr::from(bad);
        };
        assert_eq!(issues_after(at), error("adoption_port/address"), "{bad:?}");
    }
}

#[test]
fn the_port_is_a_device_name_that_the_backup_line_doesnt_use() {
    let named = |name: &str| {
        let name = name.to_owned();
        issues_after(move |c| c.adoption_port.as_mut().unwrap().port = name)
    };
    assert_eq!(named(""), error("adoption_port/port"));
    assert_eq!(named("lan 4"), error("adoption_port/port"));
    assert_eq!(named("eth1"), []);
    let backup = |c: &mut SiteConfig| {
        c.wan.failover = Some(Failover {
            link: BackupLink::Port {
                port: "lan4".to_owned(),
                vlan: None,
            },
            proto: BackupProto::Dhcp,
        });
    };
    assert_eq!(issues_after(backup), error("adoption_port/port"));
}
