//! The site as a file: an export imports back unchanged, onto the same controller or another
//! with the same nodes under other ids; secrets from the file, kept or missing; files refused.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uniwrt_core::{MacAddr, NodeId, RevisionId, Timestamp};
use zeroize::Zeroizing;

use super::{
    FileError, FileNode, Head, SecretSource, SiteFile, SiteImport, exported_secrets, fill_secret,
    import,
};
use crate::{
    node::{Node, NodeMode, NodeRole},
    site::{
        SecretRef, SiteConfig,
        migrate::upgrade,
        node::RawValue,
        validate::{Severity, has_errors},
    },
};

const HOME: &str = "wlans/home/passphrase";
const THINGS: &str = "wlans/things/passphrase";
const MESH: &str = "mesh/key";
const WAN: &str = "wan/password";

fn head() -> SiteConfig {
    let mut site =
        upgrade(serde_json::from_str(include_str!("../../fixtures/site/v11.json")).unwrap())
            .unwrap();
    // A raw secret, as brownfield import keeps a WireGuard key.
    site.nodes[1].raw[0].options.insert(
        "key".to_owned(),
        RawValue::Secret {
            secret: SecretRef::numbered(0xabc),
        },
    );
    site
}

fn node(id: i64, mac: u8, name: &str) -> Node {
    Node {
        id: NodeId::new(id),
        name: name.to_owned(),
        mac: MacAddr::new([0x02, 0, 0, 0, 0, mac]),
        board_name: "test,board".to_owned(),
        host: format!("10.0.0.{mac}"),
        port: 22,
        role: if id == 1 {
            NodeRole::Gateway
        } else {
            NodeRole::Ap
        },
        mode: NodeMode::Managed,
        created_at: Timestamp::from_unix_ms(0),
        updated_at: Timestamp::from_unix_ms(0),
    }
}

fn adopted() -> Vec<Node> {
    vec![node(1, 1, "gateway"), node(2, 2, "garden")]
}

fn values() -> BTreeMap<String, Zeroizing<String>> {
    [
        (HOME, "home passphrase"),
        (THINGS, "things passphrase"),
        (MESH, "mesh key 123"),
        (WAN, "pppoe-secret"),
        ("nodes/2/raw/0/key", "wg private key"),
    ]
    .into_iter()
    .map(|(path, value)| (path.to_owned(), Zeroizing::new(value.to_owned())))
    .collect()
}

fn exported(site: &SiteConfig, nodes: &[Node]) -> SiteFile {
    let file = SiteFile::new(
        RevisionId::new(7),
        site,
        nodes,
        Timestamp::from_unix_ms(1_000),
        "1.2.3",
        Some("armored".to_owned()),
    )
    .unwrap();
    // Through JSON, as a file travels.
    SiteFile::parse(serde_json::to_value(&file).unwrap()).unwrap()
}

fn sources(import: &SiteImport) -> Vec<(&str, SecretSource)> {
    import
        .preview
        .secrets
        .iter()
        .map(|secret| (secret.path.as_str(), secret.source))
        .collect()
}

#[test]
fn an_export_names_its_nodes_and_exports_their_secrets() {
    let mut site = head();
    // Overrides of a node no longer adopted can't be named: left out.
    let mut gone = site.nodes[1].clone();
    gone.node = NodeId::new(9);
    site.nodes.push(gone);
    let file = exported(&site, &adopted());
    assert_eq!(file.format, "uniwrt-site");
    assert_eq!(file.version, 1);
    assert_eq!(
        file.nodes
            .iter()
            .map(|n| (n.node, n.name.as_str()))
            .collect::<Vec<_>>(),
        [(NodeId::new(1), "gateway"), (NodeId::new(2), "garden")]
    );
    assert_eq!(file.config["nodes"].as_array().unwrap().len(), 2);
    let paths: Vec<String> = exported_secrets(&site, &adopted())
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(paths, [HOME, THINGS, MESH, WAN, "nodes/2/raw/0/key"]);
}

#[test]
fn an_export_imports_back_unchanged() {
    let head = head();
    let current = values();
    let at = Head {
        config: &head,
        values: &current,
    };
    // With its secrets, the same values keep their references.
    let with = import(exported(&head, &adopted()), Some(values()), at, &adopted()).unwrap();
    assert_eq!(with.site, head);
    assert!(with.values.is_empty());
    assert_eq!(with.preview.changes, []);
    assert!(!has_errors(&with.preview.issues));
    assert!(with.preview.file.opened && with.preview.file.secrets);
    assert!(sources(&with).iter().all(|(_, s)| *s == SecretSource::File));
    // Without them, every one is kept.
    let without = import(exported(&head, &adopted()), None, at, &adopted()).unwrap();
    assert_eq!(without.site, head);
    assert!(!without.preview.file.opened);
    assert!(
        sources(&without)
            .iter()
            .all(|(_, s)| *s == SecretSource::Kept)
    );
}

#[test]
fn another_controller_maps_the_nodes_by_mac() {
    let file = exported(&head(), &adopted());
    // A new host: the garden AP is node 5 there, the gateway isn't adopted, another AP is.
    let mut there = SiteConfig::defaults(None);
    let mut shed = head().nodes[0].clone();
    shed.node = NodeId::new(4);
    there.nodes.push(shed.clone());
    let nodes = [node(4, 4, "shed"), node(5, 2, "garden-ap")];
    let none = BTreeMap::new();
    let at = Head {
        config: &there,
        values: &none,
    };
    let imported = import(file, Some(values()), at, &nodes).unwrap();
    let ids: Vec<NodeId> = imported.site.nodes.iter().map(|n| n.node).collect();
    assert_eq!(ids, [NodeId::new(4), NodeId::new(5)]);
    assert_eq!(
        imported.site.nodes[0], shed,
        "the node the file doesn't name keeps its own"
    );
    assert_eq!(imported.preview.kept, [NodeId::new(4)]);
    let matches: Vec<(NodeId, Option<NodeId>)> = imported
        .preview
        .nodes
        .iter()
        .map(|m| (m.file_node, m.node))
        .collect();
    assert_eq!(
        matches,
        [
            (NodeId::new(1), None),
            (NodeId::new(2), Some(NodeId::new(5)))
        ]
    );
    // Every secret is new there: sealed from the file, the raw one under its node's new id.
    assert_eq!(
        sources(&imported),
        [
            (HOME, SecretSource::File),
            (THINGS, SecretSource::File),
            (MESH, SecretSource::File),
            (WAN, SecretSource::File),
            ("nodes/5/raw/0/key", SecretSource::File),
        ]
    );
    let raw = imported.site.secret("nodes/5/raw/0/key").unwrap();
    assert_eq!(imported.values[raw].as_str(), "wg private key");
    assert_eq!(imported.values.len(), 5);
    // No reference of the file survives: each is a placeholder with its value.
    for slot in imported.site.secrets() {
        if let Some(value) = slot.value {
            assert!(imported.values.contains_key(value), "{}", slot.path);
        }
    }
    assert!(
        imported
            .preview
            .changes
            .iter()
            .any(|c| c.path.first().is_some_and(|p| p == "wlans"))
    );
}

#[test]
fn missing_secrets_are_cleared_and_can_be_given() {
    let file = exported(&head(), &adopted());
    let mut there = head();
    there.wlans[0].passphrase = None;
    there.wan = SiteConfig::defaults(None).wan;
    let none = BTreeMap::new();
    let at = Head {
        config: &there,
        values: &none,
    };
    let mut imported = import(file, None, at, &adopted()).unwrap();
    assert_eq!(
        sources(&imported),
        [
            (HOME, SecretSource::Missing),
            (THINGS, SecretSource::Kept),
            (MESH, SecretSource::Kept),
            (WAN, SecretSource::Missing),
            ("nodes/2/raw/0/key", SecretSource::Kept),
        ]
    );
    assert_eq!(imported.site.wlans[0].passphrase, None);
    let errors: Vec<&str> = imported
        .preview
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| i.path.as_str())
        .collect();
    assert!(errors.contains(&HOME), "{errors:?}");
    // Too short for Wi-Fi, then right.
    let short = fill_secret(&mut imported, HOME, Zeroizing::new("short".to_owned()));
    assert_eq!(short.unwrap_err().path, HOME);
    fill_secret(
        &mut imported,
        HOME,
        Zeroizing::new("long enough".to_owned()),
    )
    .unwrap();
    let placeholder = imported.site.secret(HOME).unwrap();
    assert_eq!(imported.values[placeholder].as_str(), "long enough");
    assert_eq!(sources(&imported)[0], (HOME, SecretSource::File));
    assert!(
        fill_secret(
            &mut imported,
            "wlans/nope/passphrase",
            Zeroizing::new("long enough".to_owned())
        )
        .is_err()
    );
}

#[test]
fn a_raw_secret_without_a_value_goes() {
    let file = exported(&head(), &adopted());
    let mut there = head();
    there.nodes[1].raw[0].options.remove("key");
    let none = BTreeMap::new();
    let at = Head {
        config: &there,
        values: &none,
    };
    let imported = import(file, None, at, &adopted()).unwrap();
    assert!(!imported.site.nodes[1].raw[0].options.contains_key("key"));
    assert_eq!(
        sources(&imported)[4],
        ("nodes/2/raw/0/key", SecretSource::Missing)
    );
}

#[test]
fn a_bad_value_in_the_file_is_an_error() {
    let file = exported(&head(), &adopted());
    let mut bad = values();
    bad.insert(MESH.to_owned(), Zeroizing::new("short".to_owned()));
    let head = head();
    let none = BTreeMap::new();
    let at = Head {
        config: &head,
        values: &none,
    };
    let imported = import(file, Some(bad), at, &adopted()).unwrap();
    assert!(
        imported
            .preview
            .issues
            .iter()
            .any(|i| i.path == MESH && i.severity == Severity::Error)
    );
    assert_eq!(imported.site.mesh.key, None);
}

#[test]
fn the_file_s_references_never_meet_the_vault_s() {
    // A hand-written file whose references are placeholders and the head's, swapped.
    let head = head();
    let mut site = head.clone();
    site.wlans[0].passphrase = head.wlans[1].passphrase.clone();
    site.wlans[1].passphrase = Some(SecretRef::numbered(1));
    site.mesh.key = head.wlans[0].passphrase.clone();
    let none = BTreeMap::new();
    let at = Head {
        config: &head,
        values: &none,
    };
    let imported = import(exported(&site, &adopted()), None, at, &adopted()).unwrap();
    // Each slot keeps what the head holds at its path, whatever the file named.
    assert_eq!(imported.site, head);
}

#[test]
fn files_it_can_t_read_are_refused() {
    let file = serde_json::to_value(exported(&head(), &adopted())).unwrap();
    let with = |edit: fn(&mut Value)| {
        let mut file = file.clone();
        edit(&mut file);
        SiteFile::parse(file)
    };
    assert!(matches!(
        SiteFile::parse(json!({"schema": 11})),
        Err(FileError::NotSiteFile)
    ));
    assert!(matches!(
        with(|f| f["version"] = 2.into()),
        Err(FileError::TooNew(2))
    ));
    assert!(matches!(
        with(|f| f["nodes"] = 1.into()),
        Err(FileError::Invalid(_))
    ));
    let none = BTreeMap::new();
    let head = head();
    let at = Head {
        config: &head,
        values: &none,
    };
    let newer = with(|f| f["config"]["schema"] = 99.into()).unwrap();
    assert!(matches!(
        import(newer, None, at, &adopted()),
        Err(FileError::Schema(_))
    ));
    let mut twice = exported(&head, &adopted());
    twice.nodes.push(FileNode {
        node: NodeId::new(3),
        ..twice.nodes[0].clone()
    });
    assert!(matches!(
        import(twice, None, at, &adopted()),
        Err(FileError::DuplicateMac(_))
    ));
}

#[test]
fn an_older_schema_is_upgraded() {
    let mut file = exported(&head(), &adopted());
    file.config = serde_json::from_str(include_str!("../../fixtures/site/v1.json")).unwrap();
    file.nodes.clear();
    let head = head();
    let none = BTreeMap::new();
    let at = Head {
        config: &head,
        values: &none,
    };
    let imported = import(file, None, at, &adopted()).unwrap();
    assert_eq!(imported.preview.file.schema, 1);
    assert_eq!(imported.site.schema, crate::site::SCHEMA_VERSION);
}
