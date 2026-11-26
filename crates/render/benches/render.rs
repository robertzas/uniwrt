//! Render (docs/dev/TESTING.md, Performance; D-149): one node's packages from the site, as each
//! apply's plan and each drift check render every managed node. The golden tests' fixtures
//! (`tests/support`): the home site (VLAN networks, WLANs, forwards) on a DSA gateway, a DSA
//! access point and a tri-band one, a swconfig gateway on PPPoE, and OpenWrt's defaults.
//!
//! `cargo bench -p uniwrt-render` measures; `just profile bench render` writes flamegraphs.

// A bench and its fixtures may panic on what they set up.
#![allow(clippy::unwrap_used)]

// The golden tests' fixtures (which allow the parts a binary leaves unused).
#[path = "../tests/support/mod.rs"]
mod support;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use uniwrt_core::NodeId;
use uniwrt_model::{facts::NodeFacts, node::NodeRole, site::SiteConfig};

use support::{
    AP, GATEWAY, defaults_site, gateway_facts, home_site, pppoe_site, run, swconfig_facts,
    triband_facts,
};

fn render(c: &mut Criterion) {
    let cases: [(&str, SiteConfig, NodeId, NodeRole, NodeFacts); 5] = [
        (
            "gateway_home",
            home_site(),
            GATEWAY,
            NodeRole::Gateway,
            gateway_facts(),
        ),
        ("ap_home", home_site(), AP, NodeRole::Ap, gateway_facts()),
        (
            "ap_triband_home",
            home_site(),
            AP,
            NodeRole::Ap,
            triband_facts(),
        ),
        (
            "gateway_swconfig_pppoe",
            pppoe_site(),
            GATEWAY,
            NodeRole::Gateway,
            swconfig_facts(),
        ),
        (
            "gateway_defaults",
            defaults_site(),
            GATEWAY,
            NodeRole::Gateway,
            gateway_facts(),
        ),
    ];
    let mut group = c.benchmark_group("render");
    for (name, site, node, role, facts) in &cases {
        assert!(run(site, *node, *role, facts).is_ok(), "{name} renders");
        group.bench_function(*name, |b| {
            b.iter(|| run(black_box(site), *node, *role, black_box(facts)));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = uniwrt_bench::criterion();
    targets = render
}
criterion_main!(benches);
