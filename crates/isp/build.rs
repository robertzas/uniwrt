//! Embeds every preset under `data/`: adding one needs no code change (features/wan.md).

use std::{fmt::Write as _, path::Path}; // arch-check: allow (build script, runs at compile time)

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    println!("cargo:rerun-if-changed={}", dir.display()); // arch-check: allow (cargo directive)
    let mut files: Vec<_> = std::fs::read_dir(&dir) // arch-check: allow (build script)
        .expect("crates/isp/data exists")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    let mut out = String::from(
        "/// Every preset file: its stem and its text.\npub(crate) const FILES: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("UTF-8 names");
        writeln!(
            out,
            "    ({stem:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .expect("writing to a String");
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("files.rs"); // arch-check: allow (build script)
    std::fs::write(dest, out).expect("OUT_DIR is writable"); // arch-check: allow (build script)
}
