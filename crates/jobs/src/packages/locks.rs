//! What keeps an installed package in place (D-139): what the node's configuration requires
//! (render's requirements for the latest revision, D-076) and what is essential to it
//! ([`uniwrt_model::packages::essential`]).

use std::collections::{BTreeMap, BTreeSet};

use uniwrt_model::{
    facts::NodeFacts,
    node::{Node, NodeRole},
    packages::{Needs, PackageLock, essential},
    site::SiteConfig,
};
use uniwrt_render::Target;
use uniwrt_uci::Package;

use crate::{apply::survey, runner::Shared};

/// What `node` can't do without, from its facts and the site.
pub(crate) fn needs(node: &Node, facts: &NodeFacts, site: Option<&SiteConfig>) -> Needs {
    Needs {
        gateway: node.role == NodeRole::Gateway,
        radios: !facts.radios.is_empty(),
        mesh: site.is_some_and(|site| site.mesh.enabled),
    }
}

/// The packages `site` requires on `node` as render writes it, by name; why they can't be
/// told when it doesn't render. No site, no requirement.
pub(crate) async fn required(
    shared: &Shared,
    node: &Node,
    site: Option<&SiteConfig>,
    facts: &NodeFacts,
    current: &BTreeMap<String, Package>,
) -> Result<BTreeSet<String>, String> {
    let Some(site) = site else {
        return Ok(BTreeSet::new());
    };
    let secrets = survey::secrets(shared, site)
        .await
        .map_err(|err| err.message)?;
    let target = Target {
        node: node.id,
        role: node.role,
        facts,
        current,
        controller: None,
        backhaul: &[],
    };
    match uniwrt_render::render(site, &secrets, target) {
        Ok(rendered) => Ok(rendered
            .requires
            .iter()
            .map(|requirement| requirement.package.to_owned())
            .collect()),
        Err(issues) => {
            let issues: Vec<String> = issues.into_iter().map(|issue| issue.message).collect();
            Err(format!(
                "Its configuration doesn't render ({}), so which packages it needs is unknown",
                issues.join("; ")
            ))
        }
    }
}

/// Why `name` can't be removed, if it can't.
pub(crate) fn lock(name: &str, needs: Needs, required: &BTreeSet<String>) -> Option<PackageLock> {
    if required.contains(name) {
        Some(PackageLock::Required)
    } else if essential(name, needs) {
        Some(PackageLock::Essential)
    } else {
        None
    }
}
