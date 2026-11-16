//! What an official image needs once it runs (D-122, D-124): the plan's packages it lacks
//! installed, the defaults the node had removed removed again. wpads are never swapped by the
//! package manager (they conflict, and the swap cuts a mesh node off): the full one goes in
//! through the wpad swap (D-089).

use std::collections::BTreeSet;

/// The package steps after an official image.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AfterImage {
    pub(crate) install: Vec<String>,
    pub(crate) remove: Vec<String>,
    /// The plan wants the full wpad, which isn't installed.
    pub(crate) full_wpad: bool,
}

/// `wanted` (the plan's names, `-name` to remove) against what the new image has installed.
pub(crate) fn after_image(wanted: &[String], installed: &BTreeSet<String>) -> AfterImage {
    let mut plan = AfterImage::default();
    for entry in wanted {
        let (remove, name) = match entry.strip_prefix('-') {
            Some(name) => (true, name),
            None => (false, entry.as_str()),
        };
        if name.is_empty() {
            continue;
        }
        if is_wpad(name) {
            plan.full_wpad |= !remove && full_wpad(name) && !installed.contains(name);
            continue;
        }
        match (remove, installed.contains(name)) {
            (false, false) => plan.install.push(name.to_owned()),
            (true, true) => plan.remove.push(name.to_owned()),
            _ => {}
        }
    }
    plan
}

fn is_wpad(name: &str) -> bool {
    name == "wpad" || name.starts_with("wpad-")
}

/// The full wpad (802.11r/v/k, mesh): not the basic, mesh-only or mini one.
fn full_wpad(name: &str) -> bool {
    !["basic", "mesh", "mini"]
        .iter()
        .any(|variant| name.contains(variant))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn missing_packages_go_in_and_removed_defaults_go_again() {
        let installed: BTreeSet<String> = ["base-files", "luci", "ppp", "wpad-basic-mbedtls"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let wanted = names(&[
            "luci",
            "kmod-batman-adv",
            "batctl-full",
            "-ppp",
            "-odhcpd",
            "wpad-mbedtls",
            "-wpad-basic-mbedtls",
            "",
        ]);
        assert_eq!(
            after_image(&wanted, &installed),
            AfterImage {
                install: names(&["kmod-batman-adv", "batctl-full"]),
                remove: names(&["ppp"]),
                full_wpad: true,
            }
        );
    }

    #[test]
    fn a_basic_or_installed_wpad_needs_no_swap() {
        let installed: BTreeSet<String> = std::iter::once("wpad-mbedtls".to_owned()).collect();
        assert!(!after_image(&names(&["wpad-mbedtls"]), &installed).full_wpad);
        let none = BTreeSet::new();
        assert!(!after_image(&names(&["wpad-mesh-mbedtls"]), &none).full_wpad);
        assert!(after_image(&names(&["wpad-openssl"]), &none).full_wpad);
    }
}
