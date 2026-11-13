//! The packages a zero-touch image adds to its profile's defaults (pure; D-104): the mesh
//! essentials, the same for every role, since a gateway meshes too. On a device with Wi-Fi, the
//! full wpad replaces the basic one (802.11s with SAE, 802.11r and 802.11v need it); every
//! device gets batman-adv and `batctl-default` (the agent reads its JSON queries). What only
//! some settings need (SQM, mwan3, usteer) the apply installs when they ask for it.

use crate::catalog::ProfileInfo;

/// The TLS libraries wpad is built against.
const TLS: [&str; 3] = ["mbedtls", "wolfssl", "openssl"];
/// The mesh's packages.
const MESH: [&str; 2] = ["kmod-batman-adv", "batctl-default"];

/// The packages to add (and, with a leading `-`, to remove), in ASU's form.
pub(crate) fn packages(info: &ProfileInfo) -> Vec<String> {
    let mut packages = Vec::new();
    let full = |tls: &str| format!("wpad-{tls}");
    let has_full =
        info.packages.contains("wpad") || TLS.iter().any(|tls| info.packages.contains(&full(tls)));
    if !has_full {
        for wpad in &info.packages {
            let tls = wpad
                .strip_prefix("wpad-basic-")
                .or_else(|| wpad.strip_prefix("wpad-mesh-"))
                .filter(|tls| TLS.contains(tls));
            if let Some(tls) = tls {
                packages.push(format!("-{wpad}"));
                packages.push(full(tls));
            }
        }
    }
    for package in MESH {
        let batctl =
            package.starts_with("batctl") && info.packages.iter().any(|p| p.starts_with("batctl-"));
        if !info.packages.contains(package) && !batctl {
            packages.push(package.to_owned());
        }
    }
    packages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(packages: &[&str]) -> ProfileInfo {
        ProfileInfo {
            packages: packages.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    #[test]
    fn a_device_with_the_basic_wpad_gets_the_full_one_and_the_mesh() {
        assert_eq!(
            packages(&info(&["dnsmasq", "wpad-basic-mbedtls"])),
            [
                "-wpad-basic-mbedtls",
                "wpad-mbedtls",
                "kmod-batman-adv",
                "batctl-default"
            ]
        );
        assert_eq!(
            packages(&info(&["wpad-mesh-wolfssl"])),
            [
                "-wpad-mesh-wolfssl",
                "wpad-wolfssl",
                "kmod-batman-adv",
                "batctl-default"
            ]
        );
    }

    #[test]
    fn a_device_without_wifi_gets_only_the_mesh() {
        assert_eq!(
            packages(&info(&["dnsmasq", "e2fsprogs"])),
            ["kmod-batman-adv", "batctl-default"]
        );
    }

    #[test]
    fn what_a_device_has_already_is_not_asked_for() {
        assert_eq!(
            packages(&info(&["wpad-openssl", "kmod-batman-adv", "batctl-full"])),
            Vec::<String>::new()
        );
    }
}
