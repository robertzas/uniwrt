//! Render's golden sites (`crates/render/tests/support`): what uniwrt rendered imports exactly,
//! with nothing raw and no residual, on every capability set.

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{SiteConfig, wireless::Channel},
    wifi::Band,
};
use uniwrt_render::Target;

use super::{
    Found, assert_lossless, run, summary,
    support::{
        AP, GATEWAY, defaults_site, gateway_facts, home_site, pppoe_site, secrets, stock,
        swconfig_facts, triband_facts, x86_facts,
    },
};
use crate::Fate;

fn round_trip(site: &SiteConfig, nodes: &[(NodeId, NodeRole, NodeFacts)]) {
    let current = stock();
    let found: Vec<Found> = nodes
        .iter()
        .map(|(node, role, facts)| {
            let rendered = uniwrt_render::render(
                site,
                &secrets(),
                Target {
                    node: *node,
                    role: *role,
                    facts,
                    current: &current,
                },
            )
            .expect("the golden site renders");
            Found {
                node: *node,
                role: *role,
                facts: facts.clone(),
                packages: rendered
                    .packages
                    .into_values()
                    .map(|p| (p.name.clone(), p))
                    .collect(),
            }
        })
        .collect();
    let import = run(&SiteConfig::defaults(None), &found);
    assert_lossless(&import);
    for node in &import.nodes {
        let exact = node.residual.is_empty()
            && node
                .sections
                .iter()
                .all(|s| matches!(&s.fate, Fate::Mapped { kept } if kept.is_empty()));
        assert!(exact, "imports exactly:\n{}", summary(&import));
        assert_eq!(node.confidence, 100);
    }
}

#[test]
fn the_defaults_import_exactly() {
    round_trip(
        &defaults_site(),
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
    );
}

#[test]
fn the_home_site_imports_exactly() {
    round_trip(
        &home_site(),
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, gateway_facts()),
        ],
    );
}

#[test]
fn a_triband_mesh_imports_exactly() {
    let mut site = home_site();
    for policy in &mut site.radio.bands {
        if policy.band == Band::G5 {
            policy.channel = Channel::Fixed(149);
        }
    }
    round_trip(
        &site,
        &[
            (GATEWAY, NodeRole::Gateway, gateway_facts()),
            (AP, NodeRole::Ap, triband_facts()),
        ],
    );
}

#[test]
fn swconfig_and_pppoe_import_exactly() {
    round_trip(
        &pppoe_site(),
        &[
            (GATEWAY, NodeRole::Gateway, swconfig_facts()),
            (AP, NodeRole::Ap, swconfig_facts()),
        ],
    );
}

#[test]
fn a_pc_router_imports_exactly() {
    round_trip(
        &defaults_site(),
        &[(GATEWAY, NodeRole::Gateway, x86_facts())],
    );
}
