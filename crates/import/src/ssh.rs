//! SSH hardening (features/security.md, Node hardening; D-150): a node whose every `dropbear`
//! section turns passwords off for everyone and root alike is key-only, and one whose every
//! section is bound (`DirectInterface`) to the interface the management network became answers
//! there alone. Anything in between stays off, and render's `on` shows in the residual.

use uniwrt_model::site::{SiteConfig, node::SshHardening};
use uniwrt_uci::Section;

use crate::{Source, read};

pub(crate) fn import(
    site: &SiteConfig,
    source: &Source<'_>,
    renames: &read::Renames,
) -> SshHardening {
    let sections: Vec<&Section> = read::sections(source.packages, "dropbear", "dropbear").collect();
    if sections.is_empty() {
        return SshHardening::default();
    }
    let off = |s: &Section, option: &str| read::flag(s, option) == Some(false);
    let mgmt = uniwrt_render::management_network(site).map_or("lan", |n| n.id.as_str());
    SshHardening {
        key_only: sections
            .iter()
            .all(|s| off(s, "PasswordAuth") && off(s, "RootPasswordAuth")),
        management_only: sections.iter().all(|s| {
            s.option("DirectInterface")
                .is_some_and(|iface| renames.network(iface) == mgmt)
        }),
    }
}
