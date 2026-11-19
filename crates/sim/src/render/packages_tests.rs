//! The package manager UI's commands on a simulated node, for apk and opkg.

use super::{
    commands::{Disk, all},
    packages::exec,
};

fn run(argv: &[&str], manager: &str, disk: &mut Disk) -> (bool, String) {
    let out = exec(argv, manager, disk).expect("answered");
    let said = [out.stdout, out.stderr].concat();
    (out.status.success(), String::from_utf8(said).unwrap())
}

fn version<'a>(disk: &'a Disk, name: &str) -> Option<&'a str> {
    all(disk).find(|(n, ..)| *n == name).map(|(_, v, _)| v)
}

#[test]
fn the_catalog_and_the_upgrades_read_as_the_node_s_manager_writes_them() {
    let mut disk = Disk::default();
    let argv = uniwrt_node::pkg::catalog::catalog(uniwrt_model::facts::PackageManager::Apk);
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let (ok, text) = run(&argv, "apk", &mut disk);
    assert!(ok);
    let read = uniwrt_node::pkg::catalog::parse_catalog(&text);
    let tcpdump = read.iter().find(|p| p.name == "tcpdump").unwrap();
    assert_eq!(tcpdump.installed_bytes, Some(890_772));
    assert!(read.iter().any(|p| p.name == "sqm-scripts"));
    let argv = uniwrt_node::pkg::catalog::catalog(uniwrt_model::facts::PackageManager::Opkg);
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let (_, text) = run(&argv, "opkg", &mut disk);
    assert_eq!(
        uniwrt_node::pkg::catalog::parse_catalog(&text).len(),
        read.len()
    );
    let (_, text) = run(&["opkg", "list-upgradable"], "opkg", &mut disk);
    assert!(text.contains("dnsmasq - 2.91-r1 - 2.91-r2"), "{text}");
}

#[test]
fn installs_bring_their_dependencies_and_removals_take_them_along() {
    for manager in ["apk", "opkg"] {
        let mut disk = Disk::default();
        let (add, del) = if manager == "apk" {
            ("add", "del")
        } else {
            ("install", "remove")
        };
        let (ok, _) = run(&[manager, add, "tcpdump"], manager, &mut disk);
        assert!(ok);
        assert_eq!(version(&disk, "libpcap1"), Some("1.10.6-r1"));
        assert!(
            !all(&disk).find(|(n, ..)| *n == "libpcap1").unwrap().2,
            "a dependency"
        );
        // libpcap1 can't go while tcpdump needs it.
        let flag = if manager == "apk" { "-s" } else { "--noaction" };
        let dry = uniwrt_node::pkg::dry_run::parse(
            &run(&[manager, del, flag, "libpcap1"], manager, &mut disk).1,
        );
        assert_eq!(dry.kept.len(), 1, "{manager}");
        let dry = uniwrt_node::pkg::dry_run::parse(
            &run(&[manager, del, flag, "tcpdump"], manager, &mut disk).1,
        );
        assert_eq!(dry.removed, ["tcpdump", "libpcap1"], "{manager}");
        let (ok, _) = run(&[manager, del, "tcpdump"], manager, &mut disk);
        assert!(ok);
        assert_eq!(version(&disk, "tcpdump"), None);
        assert_eq!(version(&disk, "libpcap1"), None);
        // An image package goes too.
        run(&[manager, del, "rpcd"], manager, &mut disk);
        assert_eq!(version(&disk, "rpcd"), None);
    }
}

#[test]
fn an_upgrade_moves_the_image_s_package_to_the_feeds_version() {
    let mut disk = Disk::default();
    let dry = uniwrt_node::pkg::dry_run::parse(
        &run(&["apk", "upgrade", "-s", "dnsmasq"], "apk", &mut disk).1,
    );
    assert_eq!(dry.upgraded, ["dnsmasq"]);
    assert_eq!(version(&disk, "dnsmasq"), Some("2.91-r1"));
    run(&["apk", "upgrade", "dnsmasq"], "apk", &mut disk);
    assert_eq!(version(&disk, "dnsmasq"), Some("2.91-r2"));
    let (_, text) = run(
        &[
            "apk",
            "query",
            "--fields",
            "name,version",
            "--upgradable",
            "*",
        ],
        "apk",
        &mut disk,
    );
    assert!(!text.contains("dnsmasq"), "{text}");
    let (ok, text) = run(&["apk", "add", "-s", "nope"], "apk", &mut disk);
    assert!(!ok);
    assert!(text.contains("no such package"));
}
