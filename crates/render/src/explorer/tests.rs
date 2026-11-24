use std::collections::BTreeMap;

use uniwrt_core::NodeId;
use uniwrt_model::{
    explorer::UciOwner,
    site::{
        SecretRef,
        node::{ManagedPackage, NodeConfig, RawSection, RawValue},
    },
};
use uniwrt_uci::Value;

use super::*;

fn files(entries: &[(&str, &str)]) -> BTreeMap<String, Result<String, String>> {
    entries
        .iter()
        .map(|(name, text)| ((*name).to_owned(), Ok((*text).to_owned())))
        .collect()
}

fn plain(value: &str) -> RawValue {
    RawValue::Option(value.to_owned())
}

const WIRELESS: &str = "config wifi-iface 'default_radio0'\n\toption ssid 'home'\n\toption key 'hunter22'\n\tlist r0kh 'a,b,c'\n\tlist r0kh 'd,e,f'\n";
const UHTTPD: &str =
    "config uhttpd 'main'\n\tlist listen_http '0.0.0.0:80'\n\toption rfc1918_filter '1'\n";
const NETWORK: &str = "config interface 'lan'\n\toption proto 'static'\n\nconfig route\n\toption target '10.9.0.0/16'\n\toption gateway '192.168.1.2'\n";

#[test]
fn packages_are_parsed_in_order_with_secrets_masked() {
    let tree = tree(
        &files(&[("wireless", WIRELESS), ("uhttpd", UHTTPD)]),
        None,
        false,
    );
    assert_eq!(
        tree.packages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["uhttpd", "wireless"]
    );
    let iface = &tree.packages[1].sections[0];
    assert_eq!(
        (iface.kind.as_str(), iface.name.as_deref()),
        ("wifi-iface", Some("default_radio0"))
    );
    let options: Vec<_> = iface
        .options
        .iter()
        .map(|o| (o.name.as_str(), o.value.clone(), o.secret))
        .collect();
    assert_eq!(
        options,
        [
            ("ssid", Value::Single("home".to_owned()), false),
            ("key", Value::Single(MASKED.to_owned()), true),
            ("r0kh", Value::List(vec![MASKED.to_owned(); 2]), true),
        ]
    );
    assert!(
        tree.packages
            .iter()
            .all(|p| p.owner == UciOwner::Node && p.error.is_none())
    );
    assert_eq!(tree.unmatched, Vec::<u32>::new());
}

#[test]
fn a_file_that_can_t_be_read_says_why() {
    let mut files = files(&[("broken", "config\n")]);
    files.insert(
        "binary".to_owned(),
        Err("/etc/config/binary is not UTF-8".to_owned()),
    );
    let tree = tree(&files, None, false);
    assert_eq!(
        tree.packages[0].error.as_deref(),
        Some("/etc/config/binary is not UTF-8")
    );
    assert!(
        tree.packages[1]
            .error
            .as_deref()
            .unwrap()
            .contains("line 1"),
        "{:?}",
        tree.packages[1]
    );
    assert!(tree.packages.iter().all(|p| p.sections.is_empty()));
}

#[test]
fn owners_follow_the_site_and_the_node_s_mode() {
    let mut config = NodeConfig::new(NodeId::new(1));
    config.managed.insert(ManagedPackage::Network);
    let mut patch = RawSection::new("uhttpd", "uhttpd", Some("main".to_owned()));
    patch.merge = true;
    patch
        .options
        .insert("rfc1918_filter".to_owned(), plain("0"));
    config.raw.push(patch);
    let files = files(&[
        ("network", NETWORK),
        ("uhttpd", UHTTPD),
        ("wireless", WIRELESS),
    ]);
    let owners = |managed| {
        tree(&files, Some(&config), managed)
            .packages
            .iter()
            .map(|p| p.owner)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        owners(true),
        [UciOwner::Rendered, UciOwner::Patched, UciOwner::Node]
    );
    assert_eq!(owners(false), [UciOwner::Node; 3]);
}

#[test]
fn raw_sections_are_placed_on_the_sections_they_find() {
    let mut config = NodeConfig::new(NodeId::new(1));
    // 0: a merge by name.
    let mut merge = RawSection::new("uhttpd", "uhttpd", Some("main".to_owned()));
    merge.merge = true;
    merge.unset.insert("rfc1918_filter".to_owned());
    // 1: a merge of an anonymous section by the options it holds.
    let mut route = RawSection::new("network", "route", None);
    route.merge = true;
    route
        .matching
        .insert("target".to_owned(), plain("10.9.0.0/16"));
    route.options.insert("metric".to_owned(), plain("5"));
    // 2: a whole anonymous section, found by its plain options (its secret aside).
    let mut whole = RawSection::new("network", "route", None);
    whole
        .options
        .insert("target".to_owned(), plain("10.9.0.0/16"));
    let sealed = SecretRef::new(format!("config/{}", "a".repeat(32))).unwrap();
    whole
        .options
        .insert("key".to_owned(), RawValue::Secret { secret: sealed });
    // 3: a removal applied already; 4: a section of a package the node lacks; 5: a whole named
    // section not applied yet.
    let mut gone = RawSection::new("network", "interface", Some("guest".to_owned()));
    gone.remove = true;
    let elsewhere = RawSection::new("samba4", "samba", None);
    let new = RawSection::new("network", "interface", Some("iot".to_owned()));
    config.raw = vec![merge, route, whole, gone, elsewhere, new];
    let tree = tree(
        &files(&[("network", NETWORK), ("uhttpd", UHTTPD)]),
        Some(&config),
        true,
    );
    let overrides = |p: usize, s: usize| tree.packages[p].sections[s].overrides.clone();
    assert_eq!(overrides(0, 0), Vec::<u32>::new());
    assert_eq!(overrides(0, 1), [1, 2]);
    assert_eq!(overrides(1, 0), [0]);
    assert_eq!(tree.unmatched, [3, 4, 5]);
    // Each section says which options find an anonymous one of its type, as render's key does.
    assert_eq!(tree.packages[0].sections[1].key, None);
    assert_eq!(tree.packages[1].sections[0].key, None);
    let firewall = tree_of(
        "firewall",
        "config defaults

config zone
	option name 'lan'
",
    );
    assert_eq!(firewall.sections[0].key, Some(vec![]));
    assert_eq!(firewall.sections[1].key, Some(vec!["name".to_owned()]));
}

fn tree_of(name: &str, text: &str) -> UciTreePackage {
    tree(&files(&[(name, text)]), None, false)
        .packages
        .remove(0)
}

#[test]
fn an_anonymous_section_without_options_finds_one_without_any() {
    let mut config = NodeConfig::new(NodeId::new(1));
    config.raw.push(RawSection::new("system", "led", None));
    let files = files(&[("system", "config led\n\toption name 'wan'\n\nconfig led\n")]);
    let tree = tree(&files, Some(&config), true);
    assert_eq!(tree.packages[0].sections[1].overrides, [0]);
    assert_eq!(tree.packages[0].sections[0].overrides, Vec::<u32>::new());
}
