//! The `board` set: `system board`, `/etc/openwrt_release` (for the package architecture, which
//! ubus lacks) and `/etc/board.json` (for the label MAC and the [`layout`](super::layout)).

use std::collections::HashMap;

use serde::Deserialize;
use uniwrt_model::facts::{BoardFacts, Release};

use super::layout;

#[derive(Deserialize)]
pub(super) struct Raw {
    board: SystemBoard,
    #[serde(default)]
    release: HashMap<String, String>,
    #[serde(default)]
    board_json: Option<layout::Json>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SystemBoard {
    kernel: String,
    hostname: String,
    /// The CPU or SoC.
    system: String,
    model: String,
    board_name: String,
    rootfs_type: Option<String>,
    release: UbusRelease,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct UbusRelease {
    distribution: String,
    version: String,
    revision: String,
    target: String,
}

pub(super) fn parse(raw: Raw) -> BoardFacts {
    let Raw {
        board,
        release,
        board_json,
    } = raw;
    // ubus first; the release file fills what it lacks.
    let field = |ubus: String, key: &str| {
        if ubus.is_empty() {
            release.get(key).cloned().unwrap_or_default()
        } else {
            ubus
        }
    };
    BoardFacts {
        hostname: board.hostname,
        model: board.model,
        board_name: board.board_name,
        cpu: board.system,
        kernel: board.kernel,
        rootfs: board.rootfs_type,
        label_mac: board_json
            .as_ref()
            .and_then(|json| json.system.label_macaddr.as_deref())
            .and_then(|mac| mac.parse().ok()),
        release: Release {
            distribution: field(board.release.distribution, "DISTRIB_ID"),
            version: field(board.release.version, "DISTRIB_RELEASE"),
            revision: field(board.release.revision, "DISTRIB_REVISION"),
            target: field(board.release.target, "DISTRIB_TARGET"),
            arch: release.get("DISTRIB_ARCH").cloned().unwrap_or_default(),
        },
        layout: board_json.map(layout::parse).unwrap_or_default(),
    }
}
