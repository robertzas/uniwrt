//! The agent bundle: `node-agent/` embedded in the binary, and the bootstrap that installs it on
//! a node by hash (ARCHITECTURE.md, Node protocol). A node runs whatever is in [`DIR`] (RAM, so
//! a reboot clears it); the controller replaces it whenever its `VERSION` differs from the
//! bundle's hash, and the agent's hello must then report that hash.

mod tar;

use std::{fmt::Write as _, sync::LazyLock};

use bytes::Bytes;
use ring::digest::{SHA256, digest};

use crate::{Result, Session, exec::ExecRequest};

/// Where the agent lives on a node.
pub(crate) const DIR: &str = "/tmp/uniwrt/agent";

/// The agent's files, relative to `node-agent/`; a test keeps the list in step with the
/// directory.
const FILES: [(&str, &str); 12] = [
    ("bus.uc", include_str!("../../../node-agent/bus.uc")),
    ("collect.uc", include_str!("../../../node-agent/collect.uc")),
    (
        "collect/board.uc",
        include_str!("../../../node-agent/collect/board.uc"),
    ),
    (
        "collect/capabilities.uc",
        include_str!("../../../node-agent/collect/capabilities.uc"),
    ),
    (
        "collect/devices.uc",
        include_str!("../../../node-agent/collect/devices.uc"),
    ),
    (
        "collect/interfaces.uc",
        include_str!("../../../node-agent/collect/interfaces.uc"),
    ),
    (
        "collect/system.uc",
        include_str!("../../../node-agent/collect/system.uc"),
    ),
    ("exec.uc", include_str!("../../../node-agent/exec.uc")),
    ("main.uc", include_str!("../../../node-agent/main.uc")),
    ("ops.uc", include_str!("../../../node-agent/ops.uc")),
    ("proto.uc", include_str!("../../../node-agent/proto.uc")),
    ("util.uc", include_str!("../../../node-agent/util.uc")),
];

/// Installs the bundle from stdin: extracts it beside the current one, checks every file
/// against SHA256SUMS, then swaps it in. `/tmp/uniwrt` is made root's and private first, so no
/// other user on the node can touch the agent, and installs take turns (`flock`, which BusyBox
/// has in OpenWrt's default build). A constant: nothing is interpolated.
const INSTALL: &str = "set -e; umask 077; mkdir -p /tmp/uniwrt; chown 0:0 /tmp/uniwrt; \
    chmod 700 /tmp/uniwrt; cd /tmp/uniwrt; \
    if command -v flock >/dev/null; then exec 9>.lock; flock 9; fi; \
    rm -rf .new; mkdir .new; cd .new; tar xf -; sha256sum -c -s SHA256SUMS; cd ..; \
    rm -rf agent; mv .new agent";

static BUNDLE: LazyLock<Bundle> = LazyLock::new(Bundle::build);

/// The embedded agent, archived once per process.
#[derive(Debug)]
pub struct Bundle {
    /// SHA-256 of SHA256SUMS, which lists every file's hash: it changes with any file.
    hash: String,
    tar: Bytes,
}

impl Bundle {
    pub fn get() -> &'static Self {
        &BUNDLE
    }

    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The agent's command line on a node.
    pub fn argv(&self) -> Vec<String> {
        vec!["ucode".to_owned(), format!("{DIR}/main.uc")]
    }

    /// Installs the bundle unless the node has it already; returns whether it installed.
    pub async fn ensure(&self, session: &Session) -> Result<bool> {
        let current = session
            .exec(&ExecRequest::new(["cat", &format!("{DIR}/VERSION")]))
            .await?;
        if current.status.success() && current.stdout.trim_ascii() == self.hash.as_bytes() {
            return Ok(false);
        }
        let install = ExecRequest::new(["sh", "-c", INSTALL]).stdin(self.tar.clone());
        session
            .exec(&install)
            .await?
            .check("installing the agent")?;
        Ok(true)
    }

    fn build() -> Self {
        let mut sums = String::new();
        for (path, content) in FILES {
            let _ = writeln!(sums, "{}  {path}", sha256_hex(content.as_bytes()));
        }
        let hash = sha256_hex(sums.as_bytes());
        let mut archive = tar::Writer::default();
        let mut dirs: Vec<&str> = FILES
            .iter()
            .filter_map(|(path, _)| path.rsplit_once('/').map(|(dir, _)| dir))
            .collect();
        dirs.dedup();
        for dir in dirs {
            archive.dir(dir);
        }
        for (path, content) in FILES {
            archive.file(path, content.as_bytes());
        }
        archive.file("SHA256SUMS", sums.as_bytes());
        archive.file("VERSION", format!("{hash}\n").as_bytes());
        Self {
            hash,
            tar: archive.finish().into(),
        }
    }
}

fn sha256_hex(data: &[u8]) -> String {
    digest(&SHA256, data)
        .as_ref()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::Path, process::Command};

    use super::{Bundle, FILES, sha256_hex, tar::MAX_PATH};

    /// Every `.uc` file under `node-agent/` is embedded, and nothing else.
    #[test]
    fn the_bundle_holds_every_agent_file() {
        fn walk(dir: &Path, root: &Path, found: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, found);
                } else if path.extension().is_some_and(|ext| ext == "uc") {
                    let relative = path.strip_prefix(root).unwrap();
                    found.push(relative.to_str().unwrap().to_owned());
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../node-agent");
        let mut found = Vec::new();
        walk(&root, &root, &mut found);
        found.sort();
        let embedded: Vec<_> = FILES.iter().map(|(path, _)| path.to_owned()).collect();
        assert_eq!(embedded, found);
        assert!(FILES.iter().all(|(path, _)| path.len() <= MAX_PATH));
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// The archive extracts with the host's tar, and the sums check with its sha256sum, as the
    /// install script does on the node.
    #[test]
    fn the_archive_extracts_and_verifies() {
        let bundle = Bundle::get();
        assert_eq!(bundle.hash.len(), 64);
        let dir = tempfile::tempdir().unwrap();
        let mut tar = Command::new("tar")
            .args(["xf", "-"])
            .current_dir(dir.path())
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        tar.stdin.take().unwrap().write_all(&bundle.tar).unwrap();
        assert!(tar.wait().unwrap().success());
        let check = Command::new("sha256sum")
            .args(["-c", "--quiet", "SHA256SUMS"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(check.success());
        let version = fs::read_to_string(dir.path().join("VERSION")).unwrap();
        assert_eq!(version.trim(), bundle.hash());
        let sums = fs::read(dir.path().join("SHA256SUMS")).unwrap();
        assert_eq!(sha256_hex(&sums), bundle.hash());
        assert_eq!(
            fs::read_to_string(dir.path().join("collect/board.uc")).unwrap(),
            FILES[2].1
        );
    }
}
