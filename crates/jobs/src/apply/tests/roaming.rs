//! The 802.11r keys an apply renders with (D-085): one per WLAN that fast-roams, derived from
//! the controller's roaming key, which is made only once some WLAN needs it.

use uniwrt_model::{
    site::{
        SecretRef,
        wireless::{NodeScope, Roaming, Wlan, WlanSecurity},
    },
    wifi::Band,
};
use uniwrt_render::Secrets as _;
use uniwrt_store::VaultRepo as _;

use super::fleet::fleet;
use crate::apply::survey;

const GW: (uniwrt_model::node::NodeRole, &str) =
    (uniwrt_model::node::NodeRole::Gateway, "192.168.1.1");

fn wlan(id: &str, security: WlanSecurity, fast: Option<bool>, passphrase: u8) -> Wlan {
    Wlan {
        id: id.to_owned(),
        ssid: id.to_owned(),
        enabled: true,
        network: "lan".to_owned(),
        security,
        passphrase: security
            .needs_passphrase()
            .then(|| SecretRef::new(format!("config/{passphrase:032x}")).unwrap()),
        bands: vec![Band::G2],
        nodes: NodeScope::All,
        hidden: false,
        isolation: false,
        max_clients: None,
        schedule: None,
        roaming: Roaming { fast, hints: true },
    }
}

#[tokio::test]
async fn ft_keys_are_derived_for_the_wlans_that_fast_roam() {
    let fleet = fleet(&[GW]).await;
    let shared = &fleet.jobs.shared;
    let mut site = fleet.head().await.config;
    site.wlans = vec![wlan("home", WlanSecurity::Wpa2Wpa3, None, 1)];
    let secrets = survey::secrets(shared, &site).await.unwrap();
    assert_eq!(secrets.ft_key("home"), None);
    let stored = shared.vault.secret("controller/roaming_key").await.unwrap();
    assert!(stored.is_none(), "no WLAN fast-roams: no key is made");

    site.wlans = vec![
        wlan("home", WlanSecurity::Wpa2Wpa3, Some(true), 1),
        wlan("things", WlanSecurity::Wpa2, None, 2),
        wlan("guest", WlanSecurity::Owe, Some(true), 0),
        wlan("cams", WlanSecurity::Wpa2, Some(false), 3),
    ];
    let secrets = survey::secrets(shared, &site).await.unwrap();
    let roaming = shared.vault.roaming_key().await.unwrap();
    let derived = |id: &str, n: u8| roaming.ft_key(id, &format!("config/{n:032x}"));
    assert_eq!(secrets.ft_key("home"), Some(derived("home", 1).as_str()));
    assert_eq!(
        secrets.ft_key("things"),
        Some(derived("things", 2).as_str())
    );
    assert_eq!(secrets.ft_key("guest"), None);
    assert_eq!(secrets.ft_key("cams"), None);
    // The next survey finds the same key.
    let again = survey::secrets(shared, &site).await.unwrap();
    assert_eq!(again.ft_key("home"), secrets.ft_key("home"));
}
