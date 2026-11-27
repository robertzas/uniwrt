//! What a node's commands print (threat-model.md §4 and Discovery): the neighbour, route and
//! conntrack tables, the log read over the login, the packages, feeds, catalog, upgrades and a
//! dry run of either package manager, the WAN probe's round, the wpad swap's outcome, the
//! adoption port's reads (the lent port, the ARP table, the identity, the board) and a host
//! key. Every parser reads the same input.
#![no_main]

use std::net::Ipv4Addr;

use libfuzzer_sys::fuzz_target;
use uniwrt_core::Timestamp;
use uniwrt_model::{diag::IpFamily, facts::PackageManager};
use uniwrt_node::{
    ExecOutput, ExitStatus, HostKey, adoption_port, diag::tables, logread, pkg, probe, wpad,
};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = tables::parse_neighbors(&text);
    let _ = tables::parse_routes(&text, IpFamily::Ipv4);
    let _ = tables::parse_routes(&text, IpFamily::Ipv6);
    let _ = tables::parse_conntrack(&text);
    let _ = tables::parse_count(&text);
    let output = ExecOutput {
        status: ExitStatus::Code(0),
        stdout: data.to_vec(),
        stderr: Vec::new(),
    };
    let _ = logread::parse(&output);
    let _ = probe::parse_round(Timestamp::from_unix_secs(1_764_000_000), output);
    for manager in [PackageManager::Opkg, PackageManager::Apk] {
        let installed = pkg::parse(manager, &[text.to_string()]);
        let _ = pkg::feeds::parse(manager, &text);
        let _ = pkg::catalog::parse_upgrades(manager, &text, &installed);
    }
    let _ = pkg::catalog::parse_rom(&text);
    let _ = pkg::catalog::parse_catalog(&text);
    let _ = pkg::dry_run::parse(&text);
    let _ = wpad::Outcome::parse(&text);
    let _ = adoption_port::Lent::parse(&text);
    let _ = adoption_port::Board::parse(&text);
    let _ = adoption_port::segment_up(&text, Ipv4Addr::new(192, 168, 1, 2));
    let _ = adoption_port::neighbor(&text, Ipv4Addr::new(192, 168, 1, 1), "lan1");
    let _ = adoption_port::identity_mac(&text);
    let _ = HostKey::parse(&text);
});
