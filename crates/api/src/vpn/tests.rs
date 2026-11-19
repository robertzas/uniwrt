//! The VPN over the demo: a save makes the keys the server and its peers lack, public keys follow
//! private keys, a peer may bring its own, a peer's configuration is revealed (audited), and an
//! import without the keys makes new ones.

use axum::http::{Method, StatusCode};
use data_encoding::{BASE64, HEXLOWER};
use serde_json::{Value, json};
use uniwrt_model::{site::vpn::is_key, user::AuditAction};
use uniwrt_store::AuditRepo as _;

use super::keys::public_key;
use crate::testing::{Demo, json, request};

/// The cabin's router brings its own keypair.
const CABIN_KEY: &str = "TrMvSoP4jYQlY6RIzBgbssQqY3vxI2Pi+y71lOWWXX0=";

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

async fn save(demo: &Demo, config: &Value, secrets: Value) -> Value {
    let (_, head) = send(demo, Method::GET, "/v1/site", None).await;
    let body = json!({ "base": head["revision"]["id"], "config": config, "secrets": secrets });
    let (status, saved) = send(demo, Method::PUT, "/v1/site", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    saved
}

async fn reveal(demo: &Demo, path: &str) -> String {
    let body = json!({ "path": path });
    let (status, revealed) = send(demo, Method::POST, "/v1/site/secrets/reveal", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{revealed}");
    revealed["value"].as_str().unwrap().to_owned()
}

fn peer(id: &str, n: u8, role: &Value) -> Value {
    json!({
        "id": id, "name": format!("The {id}"), "enabled": true,
        "address": format!("10.0.250.{n}"), "public_key": "",
        "private_key": null, "preshared_key": null, "role": role,
    })
}

/// The demo's site with a server and two peers, none with keys: a phone and a cabin's router.
async fn tunneled(demo: &Demo) -> Value {
    let (_, site) = send(demo, Method::GET, "/v1/site", None).await;
    let mut config = site["config"].clone();
    let mut cabin = peer(
        "cabin",
        3,
        &json!({ "kind": "site", "networks": ["192.168.50.0/24"], "endpoint": null }),
    );
    cabin["public_key"] = json!(CABIN_KEY);
    config["vpn"]["wireguard"] = json!({
        "enabled": true, "port": 51820, "address": "10.0.250.1/24",
        "endpoint": { "kind": "host", "host": "vpn.example.com" }, "dns": true,
        "private_key": null, "public_key": null,
        "peers": [peer("phone", 2, &json!({ "kind": "client", "full_tunnel": true })), cabin],
    });
    config
}

#[tokio::test]
async fn a_save_makes_the_keys_and_public_keys_follow_private_keys() {
    let demo = Demo::start_with(false).await;
    let saved = save(&demo, &tunneled(&demo).await, json!({})).await;
    let server = &saved["config"]["vpn"]["wireguard"];
    let [phone, cabin] = [&server["peers"][0], &server["peers"][1]];
    for reference in [
        &server["private_key"],
        &phone["private_key"],
        &phone["preshared_key"],
        &cabin["preshared_key"],
    ] {
        assert!(
            reference.as_str().unwrap().starts_with("config/"),
            "{saved}"
        );
    }
    // The cabin's own keypair: its public key alone.
    assert_eq!(cabin["private_key"], Value::Null);
    assert_eq!(cabin["public_key"], CABIN_KEY);
    let private = reveal(&demo, "vpn/wireguard/private_key").await;
    assert_eq!(
        public_key(&private).as_deref(),
        server["public_key"].as_str()
    );
    let phone_private = reveal(&demo, "vpn/wireguard/peers/phone/private_key").await;
    assert_eq!(
        public_key(&phone_private).as_deref(),
        phone["public_key"].as_str()
    );
    assert!(is_key(
        &reveal(&demo, "vpn/wireguard/peers/cabin/preshared_key").await
    ));
    let revision = saved["revision"]["id"].clone();

    // Another public key beside the same private keys is set back: nothing to save.
    let mut tampered = saved["config"].clone();
    tampered["vpn"]["wireguard"]["public_key"] = json!(CABIN_KEY);
    tampered["vpn"]["wireguard"]["peers"][0]["public_key"] = json!(CABIN_KEY);
    let again = save(&demo, &tampered, json!({})).await;
    assert_eq!(again["revision"]["id"], revision);
    assert_eq!(again["config"], saved["config"]);

    // A private key set by the save gets its public key (RFC 7748's Alice).
    let hex = |s: &str| BASE64.encode(&HEXLOWER.decode(s.as_bytes()).unwrap());
    let alice = hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
    let set = json!({ "vpn/wireguard/private_key": alice });
    let rekeyed = save(&demo, &saved["config"], set).await;
    assert_eq!(
        rekeyed["config"]["vpn"]["wireguard"]["public_key"],
        hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
    );

    // The phone brings its own keypair from now on: its private key cleared, its public key given.
    let mut own = rekeyed["config"].clone();
    let own_key = "gN65BkIKy1eCE9pP1wdc8ROUtkHLF2PfAqYdyYBz6EA=";
    own["vpn"]["wireguard"]["peers"][0]["public_key"] = json!(own_key);
    let clear = json!({ "vpn/wireguard/peers/phone/private_key": null });
    let owned = save(&demo, &own, clear).await;
    let phone = &owned["config"]["vpn"]["wireguard"]["peers"][0];
    assert_eq!(phone["private_key"], Value::Null);
    assert_eq!(phone["public_key"], own_key);
}

#[tokio::test]
async fn a_peer_s_configuration_is_revealed_and_audited() {
    let demo = Demo::start_with(false).await;
    let saved = save(&demo, &tunneled(&demo).await, json!({})).await;
    let server = &saved["config"]["vpn"]["wireguard"];
    let config = |id: &str| format!("/v1/vpn/wireguard/peers/{id}/config");

    let (status, phone) = send(&demo, Method::POST, &config("phone"), None).await;
    assert_eq!(status, StatusCode::OK, "{phone}");
    assert_eq!(phone["file_name"], "phone.conf");
    assert_eq!(phone["own_key"], false);
    let conf = phone["conf"].as_str().unwrap();
    let private = reveal(&demo, "vpn/wireguard/peers/phone/private_key").await;
    let psk = reveal(&demo, "vpn/wireguard/peers/phone/preshared_key").await;
    for line in [
        format!("PrivateKey = {private}"),
        "Address = 10.0.250.2/32".to_owned(),
        "DNS = 10.0.250.1".to_owned(),
        format!("PublicKey = {}", server["public_key"].as_str().unwrap()),
        format!("PresharedKey = {psk}"),
        "Endpoint = vpn.example.com:51820".to_owned(),
        "AllowedIPs = 0.0.0.0/0, ::/0".to_owned(),
    ] {
        assert!(conf.lines().any(|l| l == line), "{line} in {conf}");
    }

    let (status, cabin) = send(&demo, Method::POST, &config("cabin"), None).await;
    assert_eq!(status, StatusCode::OK, "{cabin}");
    assert_eq!(cabin["own_key"], true);
    assert!(!cabin["conf"].as_str().unwrap().contains("\nPrivateKey ="));

    let (status, _) = send(&demo, Method::POST, &config("nope"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An IPv6 endpoint is bracketed.
    let mut config_gone = saved["config"].clone();
    config_gone["vpn"]["wireguard"]["endpoint"] = json!({ "kind": "host", "host": "2001:db8::1" });
    save(&demo, &config_gone, json!({})).await;
    let (_, phone) = send(&demo, Method::POST, &config("phone"), None).await;
    assert!(
        phone["conf"]
            .as_str()
            .unwrap()
            .contains("Endpoint = [2001:db8::1]:51820")
    );

    let audit = demo.store.audit_log(20).await.unwrap();
    let viewed = audit
        .iter()
        .filter(|entry| {
            entry.action == AuditAction::RevealSecret
                && entry
                    .detail
                    .as_deref()
                    .is_some_and(|d| d.contains("VPN peer"))
        })
        .count();
    assert_eq!(viewed, 3);
}

#[tokio::test]
async fn an_import_without_the_keys_makes_new_ones() {
    let source = Demo::start_with(false).await;
    let saved = save(&source, &tunneled(&source).await, json!({})).await;
    let (_, file) = send(&source, Method::GET, "/v1/site/export", None).await;

    let demo = Demo::start_with(false).await;
    let body = json!({ "file": file });
    let (status, preview) = send(&demo, Method::POST, "/v1/site/import/preview", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let generated: Vec<&str> = preview["secrets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["source"] == "generated")
        .map(|s| s["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        generated,
        [
            "vpn/wireguard/private_key",
            "vpn/wireguard/peers/phone/preshared_key",
            "vpn/wireguard/peers/cabin/preshared_key",
        ]
    );
    assert!(
        preview["issues"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["severity"] != "error"),
        "{preview}"
    );
    let body = json!({ "id": preview["id"] });
    let (status, site) = send(&demo, Method::POST, "/v1/site/import/accept", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{site}");
    let server = &site["config"]["vpn"]["wireguard"];
    let private = reveal(&demo, "vpn/wireguard/private_key").await;
    assert_eq!(
        public_key(&private).as_deref(),
        server["public_key"].as_str()
    );
    assert_ne!(
        server["public_key"],
        saved["config"]["vpn"]["wireguard"]["public_key"]
    );
    // The phone's private key stayed behind: it keeps its public key, as a peer with its own.
    let phone = &server["peers"][0];
    assert_eq!(phone["private_key"], Value::Null);
    assert_eq!(
        phone["public_key"],
        saved["config"]["vpn"]["wireguard"]["peers"][0]["public_key"]
    );
    assert!(is_key(
        &reveal(&demo, "vpn/wireguard/peers/phone/preshared_key").await
    ));
}
