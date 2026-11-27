//! Brownfield import (threat-model.md, Imports): the input as `uci export`'s output of a
//! gateway and of an access point holding the same packages, imported into a new site.
#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use uniwrt_core::NodeId;
use uniwrt_import::{Source, import};
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{SecretRef, SiteConfig},
};
use uniwrt_uci::parse_export;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(packages) = parse_export(text) else {
        return;
    };
    let packages: BTreeMap<_, _> = packages.into_iter().map(|p| (p.name.clone(), p)).collect();
    let facts = NodeFacts::default();
    let source = |node, role| Source {
        node: NodeId::new(node),
        role,
        facts: &facts,
        packages: &packages,
    };
    let sources = [source(1, NodeRole::Gateway), source(2, NodeRole::Ap)];
    let mut minted = 0;
    let _ = import(&SiteConfig::defaults(None), &sources, &mut || {
        minted += 1;
        SecretRef::numbered(minted)
    });
});
