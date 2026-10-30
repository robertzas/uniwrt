//! Build identity from build/meta.rs: the version and commit (injected by CI), the build time and
//! the target triple.

use std::fmt;

use serde::Serialize;

use crate::ui::{self, Source};

include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

/// What `uniwrt version` reports.
/// The build identity as the API reports it (`GET /api/v1/health`).
pub(crate) const API: uniwrt_api::BuildInfo = uniwrt_api::BuildInfo {
    version: VERSION,
    commit: COMMIT,
    built_at: BUILT_AT,
};

#[derive(Debug, Serialize)]
pub(crate) struct BuildInfo {
    pub(crate) version: &'static str,
    pub(crate) commit: &'static str,
    pub(crate) built_at: &'static str,
    pub(crate) target: &'static str,
    /// `embedded`, or `placeholder` when the binary was built without the web UI.
    pub(crate) ui: &'static str,
}

impl BuildInfo {
    pub(crate) fn current() -> Self {
        Self {
            version: VERSION,
            commit: COMMIT,
            built_at: BUILT_AT,
            target: TARGET,
            ui: match ui::EMBEDDED.source {
                Source::WebDist => "embedded",
                Source::Placeholder => "placeholder",
            },
        }
    }
}

impl fmt::Display for BuildInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "uniwrt {}", self.version)?;
        writeln!(f, "commit    {}", self.commit)?;
        writeln!(f, "built at  {}", self.built_at)?;
        writeln!(f, "target    {}", self.target)?;
        writeln!(f, "web UI    {}", self.ui)
    }
}
