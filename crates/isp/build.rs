//! Embeds every preset under `data/`: adding one needs no code change (features/wan.md).

use std::{fmt::Write as _, path::Path}; // arch-check: allow (build script, runs at compile time)

fn main() {
    // Read when it runs, not baked in when it compiles: a build script compiled for another
    // checkout of the same source (a worktree sharing `target/`) is reused for this one.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"); // arch-check: allow (build script)
    let dir = Path::new(&manifest).join("data");
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
        // Relative to the library's own manifest when it compiles, not this checkout's.
        writeln!(
            out,
            "    ({stem:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/data/{stem}.toml\"))),"
        )
        .expect("writing to a String");
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("files.rs"); // arch-check: allow (build script)
    std::fs::write(dest, out).expect("OUT_DIR is writable"); // arch-check: allow (build script)
}
