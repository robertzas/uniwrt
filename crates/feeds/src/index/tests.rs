use std::collections::BTreeSet;

use uniwrt_model::facts::PackageManager;

use super::{Check, files, hex, parse};
use crate::closure::resolve;

const OPKG: &[u8] = include_bytes!("../../fixtures/routing-24.10.4.Packages.gz");
const APK: &[u8] = include_bytes!("../../fixtures/routing-25.12.6.adb");

#[test]
fn opkg_s_index_gives_files_sizes_hashes_and_dependencies() {
    let index = parse(PackageManager::Opkg, OPKG).unwrap();
    assert_eq!(index.entries.len(), 68);
    let ahcpd = &index.entries[0];
    assert_eq!(ahcpd.name, "ahcpd");
    assert_eq!(ahcpd.version, "0.53-r3");
    assert_eq!(ahcpd.file, "ahcpd_0.53-r3_x86_64.ipk");
    assert_eq!(ahcpd.size, 23306);
    let Some(Check::Sha256(sha)) = ahcpd.check else {
        panic!("a SHA-256");
    };
    assert_eq!(
        hex(&sha),
        "f4753e9a9c23b7cd8cc76d1a0d25b5d7e32815f2c7257798534de60fe5dd6218"
    );
    assert_eq!(ahcpd.depends, [["libc"], ["ip"], ["librt"]]);
    assert_eq!(files(PackageManager::Opkg), ["Packages.gz", "Packages.sig"]);
}

#[test]
fn apk_s_binary_index_gives_files_sizes_hashes_and_dependencies() {
    let index = parse(PackageManager::Apk, APK).unwrap();
    assert_eq!(index.entries.len(), 56);
    let ahcpd = &index.entries[0];
    assert_eq!(ahcpd.name, "ahcpd");
    assert_eq!(ahcpd.file, "ahcpd-0.53-r3.apk");
    assert_eq!(ahcpd.size, 23389);
    let Some(Check::ApkControl(hash)) = ahcpd.check else {
        panic!("a control hash");
    };
    assert!(hex(&hash).starts_with("c5a5ea4a7baae6fa"), "{}", hex(&hash));
    assert_eq!(ahcpd.depends, [["ip"], ["libc"], ["librt"]]);
    assert_eq!(ahcpd.provides, ["ahcpd-any"]);
    let batctl = index
        .entries
        .iter()
        .find(|entry| entry.name == "batctl-default")
        .unwrap();
    assert_eq!(batctl.file, "batctl-default-2025.4-r4.apk");
    assert!(batctl.provides.contains(&"batctl".to_owned()));
}

#[test]
fn a_provided_name_resolves_in_both_formats() {
    for (manager, bytes) in [(PackageManager::Opkg, OPKG), (PackageManager::Apk, APK)] {
        let index = parse(manager, bytes).unwrap();
        let resolved = resolve(&[&index], ["batctl-default"]);
        let names: BTreeSet<&str> = resolved
            .files
            .iter()
            .map(|&(_, j)| index.entries[j].name.as_str())
            .collect();
        assert!(names.contains("batctl-default"), "{manager:?}: {names:?}");
        assert!(resolved.missing.is_empty());
    }
}

#[test]
fn the_writer_s_index_reads_back_as_it_was_given() {
    let read = parse(PackageManager::Apk, APK).unwrap();
    let written = super::adb::write(&read).unwrap();
    assert!(written.starts_with(b"ADBd"));
    assert_eq!(parse(PackageManager::Apk, &written).unwrap(), read);
    let empty = super::adb::write(&super::Index::default()).unwrap();
    assert_eq!(
        parse(PackageManager::Apk, &empty).unwrap(),
        super::Index::default()
    );
}

#[test]
fn a_written_package_matches_its_control_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo-1.0-r1.apk");
    let (file, hash) = super::adb::package("demo", "1.0-r1", &vec![7; 70_000]).unwrap();
    std::fs::write(&path, &file).unwrap();
    let entry = super::Entry {
        name: "demo".to_owned(),
        version: "1.0-r1".to_owned(),
        file: "demo-1.0-r1.apk".to_owned(),
        size: file.len() as u64,
        check: Some(Check::ApkControl(hash)),
        depends: Vec::new(),
        provides: Vec::new(),
    };
    assert_eq!(crate::verify::check(&entry, &path), Ok(()));
    let other = super::adb::package("demo", "1.0-r2", &vec![7; 70_000]).unwrap();
    assert_ne!(other.1, hash, "the control block names the version");
}

#[test]
fn damaged_indexes_are_errors_not_panics() {
    assert!(parse(PackageManager::Apk, b"").is_err());
    assert!(parse(PackageManager::Apk, b"ADBdgarbage").is_err());
    assert!(parse(PackageManager::Apk, b"ADBc\x02\x01").is_err());
    assert!(parse(PackageManager::Opkg, b"not gzip").is_err());
    let full = super::adb::inflate(APK).unwrap();
    for cut in [8, 12, 64, full.len() / 2] {
        let mut stream = b"ADB.".to_vec();
        stream.extend_from_slice(&full[4..cut]);
        assert!(super::adb::parse(&stream).is_err(), "cut at {cut}");
    }
}
