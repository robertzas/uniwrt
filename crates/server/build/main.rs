//! Build script: the build identity (`meta`) and the embedded web UI (`ui`), written to `$OUT_DIR`.
//!
//! Inputs: `UNIWRT_VERSION`, `UNIWRT_COMMIT`, `SOURCE_DATE_EPOCH` and `UNIWRT_WEB_DIST`
//! (docs/dev/BUILD.md). `civil` is shared with the crate's unit tests.

mod civil;
mod meta;
mod ui;

use std::{env, error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?);
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?);
    meta::generate(&out_dir)?;
    ui::generate(&manifest_dir, &out_dir)
}
