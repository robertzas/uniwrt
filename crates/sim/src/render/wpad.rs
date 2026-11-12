//! The node's wpad and the wpad swap's commands (D-089). A node set up by hand runs the mesh
//! wpad, which has 802.11v; a factory gateway runs the image's basic one, which lacks it (and
//! the mesh, EAP and WPS, [`super::system::capabilities`]). The swap's commands answer as such a
//! node would: there is room, the feeds have the full wpad, fetching and the offline check
//! succeed, and the swap is done by the first read of its result (the real one takes seconds,
//! Wi-Fi restarting).

use uniwrt_node::{ExecOutput, wpad::DIR};

use super::commands::{failed, ok};

/// The full wpad, which the swap installs.
pub(crate) const FULL: &str = "wpad-mbedtls";
/// The wpad of a node set up by hand: the mesh needs it (or the full one).
pub(crate) const MESH: &str = "wpad-mesh-mbedtls";
/// The image's wpad, on a factory gateway.
pub(crate) const BASIC: &str = "wpad-basic-mbedtls";

fn df(device: &str, size: u64, available: u64) -> Vec<u8> {
    format!(
        "Filesystem           1K-blocks      Used Available Use% Mounted on\n\
         {device:<20} {size:>9} {used:>9} {available:>9} {pct:>3}% {mount}\n",
        used = size - available,
        pct = (size - available) * 100 / size,
        mount = if device == "tmpfs" { "/tmp" } else { "/" },
    )
    .into_bytes()
}

/// Answers the swap's commands; `None` for others.
pub(crate) fn exec(argv: &[&str], manager: &str, wpad: &mut &'static str) -> Option<ExecOutput> {
    let state = format!("{DIR}/state");
    let log = format!("{DIR}/log");
    Some(match argv {
        ["df", "-k", "/"] => ok(df("overlayfs:/overlay", 13_120, 9_472)),
        ["df", "-k", "/tmp"] => ok(df("tmpfs", 245_820, 241_312)),
        ["rm", "-rf", dir] | ["mkdir", "-p", dir] if *dir == DIR => ok(Vec::new()),
        // apk's update, fetch and offline check; opkg's download.
        ["apk", "--cache-dir", dir, ..]
        | ["apk", "add", "--no-network", "--cache-dir", dir, ..]
            if *dir == DIR && manager == "apk" =>
        {
            ok(Vec::new())
        }
        ["sh", "-c", _, "sh", dir, ..] if *dir == DIR && manager == "opkg" => ok(Vec::new()),
        ["sh", "-c", _, "sh", _, tool, from, to, dir]
            if *dir == DIR && *tool == manager && from == wpad && *to == FULL =>
        {
            *wpad = FULL;
            ok(b"started\n".to_vec())
        }
        ["cat", path] if *path == state => {
            if *wpad == FULL {
                ok(b"swapped\n".to_vec())
            } else {
                failed(
                    1,
                    format!("cat: can't open '{state}': No such file or directory\n"),
                )
            }
        }
        ["cat", path] if *path == log => ok(b"result: swapped\n".to_vec()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use uniwrt_model::facts::PackageManager;
    use uniwrt_node::wpad::{self, Swap};

    use super::{BASIC, FULL, exec};

    /// The job's commands, in its order, each answered.
    #[test]
    fn the_swap_installs_the_full_wpad() {
        for (manager, name) in [(PackageManager::Apk, "apk"), (PackageManager::Opkg, "opkg")] {
            let mut installed = BASIC;
            let swap = Swap {
                manager,
                from: BASIC.to_owned(),
                to: FULL.to_owned(),
            };
            let mut run = |argv: &[String]| {
                let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                exec(&argv, name, &mut installed).unwrap_or_else(|| panic!("{argv:?}"))
            };
            for path in ["/", "/tmp"] {
                let df = run(&["df".to_owned(), "-k".to_owned(), path.to_owned()]);
                let text = String::from_utf8(df.stdout).unwrap();
                assert!(wpad::available_kib(&text).unwrap() > wpad::FLASH_KIB);
            }
            let state = vec!["cat".to_owned(), format!("{}/state", wpad::DIR)];
            assert!(!run(&state).status.success());
            // `opkg update` is the feeds' (`super::super::feeds`).
            for argv in wpad::fetch(&swap)
                .iter()
                .filter(|argv| *argv != &["opkg", "update"])
            {
                assert!(run(argv).status.success(), "{argv:?}");
            }
            assert_eq!(run(&wpad::launch(&swap)).stdout, b"started\n");
            assert_eq!(run(&state).stdout, b"swapped\n");
            assert_eq!(installed, FULL);
        }
    }
}
