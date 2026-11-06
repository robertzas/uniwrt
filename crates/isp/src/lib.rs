//! ISP presets (features/wan.md, Preset format; D-075): what the WAN wizard offers after the
//! country, and what the connectivity test says when a step fails.
//!
//! - **Presets:** one TOML file per preset under `crates/isp/data/`, embedded at build time
//!   (`build.rs` lists the directory), so adding one needs no code change. [`presets`] parses
//!   them once, sorted generic first, then by country and name; `cargo test` loads and checks
//!   every file (its `id` is its file name, its values valid, its prompts complete, a filled-in
//!   copy passes the site's validation). A [`Preset`] says how IPv4 and IPv6 arrive and which
//!   [`Prompt`]s the user answers; [`Preset::wan`] turns it and the [`Answers`] into the site's
//!   [`WanConfig`](uniwrt_model::site::gateway::WanConfig). IPv6 `auto` means DHCPv6 with prefix
//!   delegation, and 6rd ([`Preset::fallback`]) if no prefix arrives: the connectivity test
//!   decides, and the wizard saves whichever worked.
//! - **Steps and hints:** the connectivity test's [`Step`]s in order, and the [`hint`] for a
//!   failed one, a preset's own before the generic one.
//!
//! Pure: no IO, no clock (L0).

mod files {
    include!(concat!(env!("OUT_DIR"), "/files.rs"));
}
mod preset;
mod step;
#[cfg(test)]
mod tests;

use std::sync::OnceLock;

pub use preset::{
    Answers, Ipv4Kind, Ipv6Kind, Preset, PresetIpv4, PresetIpv6, Prompt, SixRd, Status,
};
pub use step::{Step, hint};

/// Every embedded preset that parses (all do: the tests check), generic ones first, then by
/// country and name.
pub fn presets() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        let mut presets: Vec<Preset> = files::FILES
            .iter()
            .filter_map(|(_, text)| toml::from_str(text).ok())
            .collect();
        presets.sort_by(|a, b| {
            (&a.country, &a.name, &a.variant).cmp(&(&b.country, &b.name, &b.variant))
        });
        presets
    })
}

/// The preset with this id.
pub fn preset(id: &str) -> Option<&'static Preset> {
    presets().iter().find(|p| p.id == id)
}
