//! Round trips (D-072): the simulated fleet's hand-made configs, OpenWrt's stock ones and the
//! hwsim lab's gateway import with nothing lost beyond the residual, and render's golden
//! sites import exactly.

#[allow(dead_code, unreachable_pub, clippy::all)]
#[path = "../../../render/tests/support/mod.rs"]
mod support;

mod firewall;
mod golden;
mod sim;
mod stock;

use std::{collections::BTreeMap, fmt::Write as _};

use uniwrt_core::NodeId;
use uniwrt_model::{
    facts::NodeFacts,
    node::NodeRole,
    site::{SecretRef, SiteConfig, validate::Severity},
};
use uniwrt_render::{RENDERABLE, Target};
use uniwrt_uci::Package;

use crate::{Fate, Import, Source};

/// A node as found: its id, role, facts and packages.
pub(crate) struct Found {
    pub node: NodeId,
    pub role: NodeRole,
    pub facts: NodeFacts,
    pub packages: BTreeMap<String, Package>,
}

pub(crate) fn parse(files: &[(&str, &str)]) -> BTreeMap<String, Package> {
    files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), uniwrt_uci::parse(name, text).unwrap()))
        .collect()
}

/// Imports into `base`, naming secrets `config/000…1`, `…2` in order.
pub(crate) fn run(base: &SiteConfig, found: &[Found]) -> Import {
    let sources: Vec<Source<'_>> = found
        .iter()
        .map(|f| Source {
            node: f.node,
            role: f.role,
            facts: &f.facts,
            packages: &f.packages,
        })
        .collect();
    let mut n = 0_u32;
    let mut mint = || {
        n += 1;
        SecretRef::new(format!("config/{n:032x}")).unwrap()
    };
    crate::import(base, &sources, &mut mint)
}

/// Asserts what every import keeps: each node renders, nothing is lost but what render's
/// design drops (each removed section is a dropped one, no option of a kept section goes), and
/// the site validates.
pub(crate) fn assert_lossless(import: &Import) {
    let nodes = import.site.nodes.iter().map(|n| n.node).collect();
    let errors: Vec<_> = uniwrt_model::site::validate::validate(&import.site, &nodes)
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "the import validates: {errors:?}");
    for node in &import.nodes {
        assert!(node.issues.is_empty(), "{}: {:?}", node.node, node.issues);
        for diff in &node.residual {
            let dropped = node
                .sections
                .iter()
                .filter(|s| s.package == diff.package && s.fate == Fate::Dropped)
                .count();
            let removed = diff.sections.iter().filter(|s| s.after.is_none()).count();
            assert_eq!(
                removed, dropped,
                "{}: only dropped sections go: {diff}",
                node.node
            );
            for section in &diff.sections {
                if section.after.is_some() {
                    assert!(
                        section.options.iter().all(|o| o.after.is_some() || {
                            let kind = section
                                .before
                                .as_ref()
                                .map_or_else(String::new, |b| b.kind.clone());
                            crate::fit::consumed(&diff.package, &kind, &o.name)
                                || crate::dumb::option_dropped(
                                    node.role,
                                    &diff.package,
                                    &kind,
                                    &o.name,
                                )
                        }),
                        "{}: no option goes: {diff}",
                        node.node
                    );
                }
            }
        }
    }
}

/// Whether a residual changes only the 802.11r keys: the import renders a stand-in for the
/// controller's, which replace a node's own (D-085).
pub(crate) fn ft_keys_only(diff: &uniwrt_uci::Diff) -> bool {
    diff.sections.iter().all(|section| {
        section.before.is_some()
            && section.after.is_some()
            && !section.moved
            && section
                .options
                .iter()
                .all(|option| matches!(option.name.as_str(), "r0kh" | "r1kh"))
    })
}

/// The packages the import renders for a node, with all of them managed.
pub(crate) fn rendered(import: &Import, found: &Found) -> BTreeMap<String, Package> {
    let mut site = import.site.clone();
    crate::node_config(&mut site, found.node).managed = RENDERABLE.into_iter().collect();
    let rendered = uniwrt_render::render(
        &site,
        &crate::fit::Trial(&import.secrets),
        Target {
            node: found.node,
            role: found.role,
            facts: &found.facts,
            current: &found.packages,
            controller: uniwrt_render::held_controller(&found.packages),
            backhaul: &[],
        },
    )
    .expect("the import renders");
    rendered
        .packages
        .into_values()
        .map(|p| (p.name.clone(), p))
        .collect()
}

/// What became of each node, for review: confidence, raw and kept sections, the residual.
pub(crate) fn summary(import: &Import) -> String {
    let mut out = String::new();
    for node in &import.nodes {
        let _ = writeln!(out, "# {} — confidence {}%", node.node, node.confidence);
        for section in &node.sections {
            let name = section.name.as_deref().unwrap_or("@");
            let renamed = section
                .renamed
                .as_ref()
                .map(|r| format!(" → {r}"))
                .unwrap_or_default();
            match &section.fate {
                Fate::Mapped { kept } if kept.is_empty() && renamed.is_empty() => continue,
                Fate::Mapped { kept } if kept.is_empty() => {}
                Fate::Mapped { kept } => {
                    let _ = write!(out, "kept ");
                    let _ = write!(out, "[{}] ", kept.join(", "));
                }
                Fate::Raw => {
                    let _ = write!(out, "raw ");
                }
                Fate::Dropped => {
                    let _ = write!(out, "dropped ");
                }
            }
            let _ = writeln!(
                out,
                "{}.{name} ({}){renamed}",
                section.package, section.kind
            );
        }
        for issue in &node.warnings {
            let _ = writeln!(out, "warning: {}: {}", issue.path, issue.message);
        }
        for diff in &node.residual {
            residual(&mut out, diff);
        }
    }
    out
}

/// A package's residual, one line per section: `-` dropped, `+` added (with its options), `~`
/// changed (each option `before → after`), `moved`.
fn residual(out: &mut String, diff: &uniwrt_uci::Diff) {
    let id = |s: &uniwrt_uci::SectionId| match &s.name {
        Some(name) => format!("{}.{name}", diff.package),
        None => format!("{}.@{}[{}]", diff.package, s.kind, s.index),
    };
    let value = |v: &Option<uniwrt_uci::Value>| match v {
        Some(v) => v
            .items()
            .iter()
            .map(|i| format!("'{i}'"))
            .collect::<Vec<_>>()
            .join(" "),
        None => "∅".to_owned(),
    };
    for section in &diff.sections {
        match (&section.before, &section.after) {
            (Some(before), None) => {
                let _ = writeln!(out, "- {}", id(before));
            }
            (None, Some(after)) => {
                let options: Vec<String> = section
                    .options
                    .iter()
                    .map(|o| format!("{}={}", o.name, value(&o.after)))
                    .collect();
                let _ = writeln!(out, "+ {} {}", id(after), options.join(" "));
            }
            (_, Some(after)) => {
                let moved = if section.moved { " (moved)" } else { "" };
                let _ = writeln!(out, "~ {}{moved}", id(after));
                for o in &section.options {
                    let _ = writeln!(
                        out,
                        "    {}: {} → {}",
                        o.name,
                        value(&o.before),
                        value(&o.after)
                    );
                }
            }
            (None, None) => {}
        }
    }
}
