//! The system harness (TESTING.md, System): rendered configs on real OpenWrt.
//!
//! - **API:** [`Node`] is an OpenWrt container booted with procd, reached through `docker exec`:
//!   its [`facts`](Node::facts) from the working tree's agent, its current
//!   [`packages`](Node::packages), [`apply`](Node::apply) (write, check that `uci` reads back
//!   what was written, `reload_config`) and netifd's and the bridge's state. [`render_for`]
//!   renders a [`site`] for a node from what it reports ([`render_with`] from adjusted facts). [`needs`] skips a test on a host
//!   whose kernel can't do what it needs (8021q, pppoe, nftables), except in CI.
//! - **Topologies:** `scripts/test-system.sh` links an ISP, a gateway and an AP (wired), and
//!   `scripts/wifi-lab.sh` the hwsim lab (Wi-Fi and the mesh); each test renders onto them.
//!   `tests/apply/` drives `uniwrt_jobs`' apply onto the wired topology over a `docker exec`
//!   `uniwrt_jobs::Dial` (logins go to the container holding the address; a container
//!   can be lost after it applies).
//! - **Invariants:** a test owns the topology while it runs (`--test-threads 1`) and renders
//!   every package it relies on, so tests don't depend on their order. Values reach a
//!   container's shell as arguments, never in a script's text.
//! - **Dependencies:** core, model, node (the agent's facts), render and uci (and jobs, store
//!   and tokio for `tests/apply/`); Docker at run
//!   time. The tests are `#[ignore]`d: the scripts run them with `--run-ignored only`.

mod docker;
pub mod site;

use std::collections::BTreeMap;

use uniwrt_core::NodeId;
use uniwrt_model::{facts::NodeFacts, node::NodeRole, site::SiteConfig};
use uniwrt_render::{Rendered, Target, render};
use uniwrt_uci::Package;

pub use self::docker::{Node, Run, has_ipv4, ipv4};

/// Renders `site` for `node` from its facts and current packages; panics on render's issues.
pub fn render_for(site: &SiteConfig, node: &Node, id: NodeId, role: NodeRole) -> Rendered {
    render_with(site, node, id, role, &node.facts())
}

/// [`render_for`] with facts the test adjusted, such as a stub WAN port.
pub fn render_with(
    site: &SiteConfig,
    node: &Node,
    id: NodeId,
    role: NodeRole,
    facts: &NodeFacts,
) -> Rendered {
    let current: BTreeMap<String, Package> = node.packages();
    render(
        site,
        &site::secrets(),
        Target {
            node: id,
            role,
            facts,
            current: &current,
            controller: None,
            backhaul: &[],
        },
    )
    .unwrap_or_else(|issues| panic!("{}: render refused: {issues:#?}", node.container))
}

/// Whether the host's kernel can do `feature` (`8021q`, `pppoe`, `nftables`, `sch_cake`), as
/// scripts/test-system.sh found (`UNIWRT_SYSTEM_LACKS`; the script fails in CI if it can't).
/// Without it a test skips, with a note.
pub fn needs(feature: &str) -> bool {
    let lacks = std::env::var("UNIWRT_SYSTEM_LACKS").unwrap_or_default();
    if lacks.split_whitespace().any(|lack| lack == feature) {
        eprintln!("skipped: the host's kernel lacks {feature}");
        return false;
    }
    true
}
