//! The port path's Prepare step (features/adoption.md, Path C, step 5; D-108). Before Configure
//! applies the revision the gateway holds, the device gets the mesh's packages its image lacks
//! where that revision turns the mesh on, since the render refuses a mesh without them:
//! batman-adv and batctl (the firmware's mesh essentials) and, where a radio on the mesh's band
//! would carry it, the full wpad for 802.11s (the wpad swap, D-089, which brings 802.11v too).
//! The swap is safe here: the device is on a cable with its Wi-Fi off. A radio-less device
//! meshes over its cables alone, so it keeps its wpad.

use std::time::Duration;

use uniwrt_model::{
    facts::{NodeFacts, Software, WifiFeature},
    job::{JobError, StepName},
    node::Node,
    site::wireless::MeshConfig,
    wifi::Band,
};
use uniwrt_node::{ExecRequest, pkg, proto::ops::Set};

use crate::{
    Remote,
    apply::survey,
    errors::{chain, last_line},
    manage,
    progress::Progress,
    runner::Shared,
    wpad,
};

/// The packages that bring batman-adv and batctl, as the agent finds them.
const MESH: [(Software, &str); 2] = [
    (Software::BatmanAdv, "kmod-batman-adv"),
    (Software::Batctl, "batctl-default"),
];
/// How long refreshing the index or installing may take: slow mirrors on slow routers.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);

/// What the device lacks for the mesh.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Needs {
    /// The feed packages to install.
    pub(super) packages: Vec<&'static str>,
    /// Whether the full wpad must come: a radio would carry the mesh and hostapd lacks 802.11s.
    pub(super) wpad: bool,
}

impl Needs {
    pub(super) fn of(mesh: &MeshConfig, facts: &NodeFacts) -> Self {
        let caps = &facts.capabilities;
        let band = mesh.band.unwrap_or(Band::G5);
        Self {
            packages: MESH
                .iter()
                .filter(|(software, _)| !caps.software.contains(software))
                .map(|&(_, package)| package)
                .collect(),
            wpad: facts.radios.iter().any(|radio| radio.band == band)
                && !caps.wifi.contains(&WifiFeature::Mesh),
        }
    }
}

/// Runs the step for `node`, managed at its lease; `Err` says why Configure can't follow
/// (the step has failed with it).
pub(super) async fn prepare(
    shared: &Shared,
    mesh: &MeshConfig,
    node: &Node,
    progress: &mut Progress,
) -> Result<(), String> {
    if !mesh.enabled {
        progress
            .skip(StepName::Prepare, "The mesh is off: nothing to bring")
            .await;
        return Ok(());
    }
    progress.start(StepName::Prepare).await;
    let result = match manage::login(shared, node).await {
        Ok(remote) => {
            let (remote, result) = bring(shared, mesh, node, remote, progress).await;
            if let Some(remote) = remote {
                remote.close().await;
            }
            result
        }
        Err(err) => Err(format!("Cannot log in to the device: {}", err.message)),
    };
    match result {
        Ok(detail) => {
            progress.done(Some(detail)).await;
            Ok(())
        }
        Err(why) => {
            progress.fail(&why).await;
            Err(why)
        }
    }
}

/// Reads what the device has, then installs and swaps what it lacks; says what it did.
async fn bring(
    shared: &Shared,
    mesh: &MeshConfig,
    node: &Node,
    remote: Box<dyn Remote>,
    progress: &mut Progress,
) -> (Option<Box<dyn Remote>>, Result<String, String>) {
    progress.note("Reading what the device has").await;
    let sets = [Set::Capabilities, Set::Radios];
    let facts = match survey::collect(shared, remote.as_ref(), &sets, &sets).await {
        Ok(facts) => facts,
        Err(why) => return (Some(remote), Err(format!("Cannot read the device: {why}"))),
    };
    let needs = Needs::of(mesh, &facts);
    let mut done = Vec::new();
    if !needs.packages.is_empty() {
        let Some(manager) = facts.capabilities.package_manager else {
            let why = "The device has neither apk nor opkg to install batman-adv with";
            return (Some(remote), Err(why.to_owned()));
        };
        let names = needs.packages.join(", ");
        progress.note(format!("Installing {names}")).await;
        if let Err(why) = install(remote.as_ref(), manager, &needs.packages).await {
            return (
                Some(remote),
                Err(format!(
                    "{why}. Does the device reach the internet and its package feeds?"
                )),
            );
        }
        done.push(format!("Installed {names}"));
    }
    let remote = if needs.wpad {
        let (remote, swapped) = wpad::within(shared, node, remote, progress).await;
        match swapped {
            Ok(true) => done.push("swapped in the full wpad".to_owned()),
            Ok(false) => {}
            Err(err) => return (remote, Err(swap_failed(&err, &done))),
        }
        remote
    } else {
        Some(remote)
    };
    let detail = if done.is_empty() {
        "The device has the mesh's packages".to_owned()
    } else {
        let mut detail = done.join(", then ");
        detail.push_str(" for the mesh");
        detail
    };
    (remote, Ok(detail))
}

async fn install(
    remote: &dyn Remote,
    manager: uniwrt_model::facts::PackageManager,
    packages: &[&str],
) -> Result<(), String> {
    for argv in [pkg::update(manager), pkg::install(manager, packages)] {
        let what = argv.join(" ");
        let mut request = ExecRequest::new(argv);
        request.timeout = INSTALL_TIMEOUT;
        request.max_output = 1 << 20;
        match remote.exec(request).await {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let text = if output.stderr.is_empty() {
                    &output.stdout
                } else {
                    &output.stderr
                };
                return Err(format!("{what} failed: {}", last_line(text)));
            }
            Err(err) => return Err(format!("{what} failed: {}", chain(&err))),
        }
    }
    Ok(())
}

fn swap_failed(err: &JobError, done: &[String]) -> String {
    let first = if done.is_empty() {
        String::new()
    } else {
        format!("{} first. ", done.join(", "))
    };
    format!("{first}The wpad swap failed: {}", err.message)
}

#[cfg(test)]
mod tests {
    use uniwrt_model::facts::{Capabilities, RadioFacts};

    use super::*;

    fn facts(software: &[Software], wifi: &[WifiFeature], bands: &[Band]) -> NodeFacts {
        NodeFacts {
            capabilities: Capabilities {
                software: software.iter().copied().collect(),
                wifi: wifi.iter().copied().collect(),
                ..Capabilities::default()
            },
            radios: bands
                .iter()
                .map(|&band| {
                    let radio = serde_json::json!({"name": "radio", "band": band, "up": true, "disabled": false, "ifaces": []});
                    serde_json::from_value::<RadioFacts>(radio).expect("a radio")
                })
                .collect(),
            ..NodeFacts::default()
        }
    }

    fn mesh(band: Option<Band>) -> MeshConfig {
        MeshConfig {
            enabled: true,
            band,
            ..MeshConfig::default()
        }
    }

    #[test]
    fn a_factory_image_needs_batman_and_the_full_wpad() {
        let factory = facts(&[], &[WifiFeature::Sae], &[Band::G2, Band::G5]);
        assert_eq!(
            Needs::of(&mesh(None), &factory),
            Needs {
                packages: vec!["kmod-batman-adv", "batctl-default"],
                wpad: true,
            }
        );
    }

    #[test]
    fn a_mesh_image_needs_nothing() {
        let ready = facts(
            &[Software::BatmanAdv, Software::Batctl],
            &[WifiFeature::Mesh],
            &[Band::G5],
        );
        let needs = Needs::of(&mesh(None), &ready);
        assert!(needs.packages.is_empty() && !needs.wpad, "{needs:?}");
    }

    #[test]
    fn the_wpad_comes_only_where_a_radio_would_carry_the_mesh() {
        let radioless = facts(&[Software::BatmanAdv], &[WifiFeature::Sae], &[]);
        assert_eq!(
            Needs::of(&mesh(None), &radioless),
            Needs {
                packages: vec!["batctl-default"],
                wpad: false,
            }
        );
        let low_band_only = facts(&[Software::BatmanAdv, Software::Batctl], &[], &[Band::G2]);
        assert!(!Needs::of(&mesh(None), &low_band_only).wpad);
        assert!(Needs::of(&mesh(Some(Band::G2)), &low_band_only).wpad);
    }
}
