//! Tailscale's endpoints: the status against the gateway's facts ([`report`]), and over the
//! demo, the login's refusals (the site's Tailscale off, an observed gateway) and a user who
//! isn't an admin.

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use uniwrt_core::Timestamp;
use uniwrt_model::{
    facts::{NodeFacts, TailscaleFacts, TailscalePrefs, TailscaleState},
    site::{SiteConfig, vpn::tailscale::Tailscale},
};

use super::report;
use crate::testing::{Demo, json, request};

fn site() -> SiteConfig {
    let mut config = SiteConfig::defaults(None);
    let mut tailscale = Tailscale::new(vec![config.networks[0].id.clone()]);
    tailscale.exit_node = true;
    config.vpn.tailscale = Some(tailscale);
    config
}

fn facts(state: TailscaleState, approved: &[&str], prefs: TailscalePrefs) -> NodeFacts {
    let mut facts = NodeFacts {
        collected_at: Timestamp::from_unix_secs(1_800_000_000),
        ..NodeFacts::default()
    };
    facts.services.tailscale = Some(Box::new(TailscaleFacts {
        state: Some(state),
        approved_routes: approved.iter().map(|r| (*r).to_owned()).collect(),
        prefs: Some(prefs),
        ..TailscaleFacts::default()
    }));
    facts
}

fn held(config: &SiteConfig) -> TailscalePrefs {
    let want = config.vpn.tailscale.as_ref().unwrap().prefs(config);
    let mut routes = want.routes;
    routes.extend(["0.0.0.0/0".to_owned(), "::/0".to_owned()]);
    TailscalePrefs {
        want_running: true,
        logged_out: false,
        advertise_routes: routes,
        hostname: None,
        accept_dns: false,
        accept_routes: false,
        netfilter_off: true,
    }
}

#[test]
fn the_status_tells_held_prefs_and_routes_awaiting_approval() {
    let config = site();
    let lan = config.vpn.tailscale.as_ref().unwrap().prefs(&config).routes[0].clone();
    // Running with the prefs, nothing approved yet: the LAN and the exit node await.
    let running = facts(TailscaleState::Running, &[], held(&config));
    let status = report(&config, Some(&running), true);
    assert!(status.prefs_applied && status.online);
    assert_eq!(
        status.unapproved_routes,
        [lan.clone(), "0.0.0.0/0".to_owned()]
    );
    // Approved: none await.
    let approved = facts(
        TailscaleState::Running,
        &[&lan, "0.0.0.0/0", "::/0"],
        held(&config),
    );
    assert_eq!(
        report(&config, Some(&approved), true).unapproved_routes,
        Vec::<String>::new()
    );
    // Logged out with the defaults: not applied, and nothing to approve while it doesn't run.
    let fresh = facts(TailscaleState::NeedsLogin, &[], TailscalePrefs::default());
    let status = report(&config, Some(&fresh), false);
    assert!(!status.prefs_applied && status.unapproved_routes.is_empty());
    // No gateway facts, or no Tailscale in the site.
    let none = report(&config, None, false);
    assert_eq!((none.daemon, none.collected_at), (None, None));
    assert!(!report(&SiteConfig::defaults(None), Some(&running), true).prefs_applied);
}

async fn send(demo: &Demo, method: Method, uri: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let response = demo
        .send(request(method, uri, body, Some(&demo.cookie)))
        .await;
    let status = response.status();
    (status, json(response).await)
}

#[tokio::test]
async fn a_login_needs_tailscale_on_and_a_managed_gateway_running_it() {
    let demo = Demo::start().await;
    let (status, body) = send(&demo, Method::GET, "/v1/vpn/tailscale", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["daemon"], Value::Null);
    let (status, body) = send(&demo, Method::POST, "/v1/vpn/tailscale/login", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["detail"].as_str().unwrap().contains("off in the site"));
    // On in the site: the demo's gateway is observed.
    let (_, head) = send(&demo, Method::GET, "/v1/site", None).await;
    let mut config = head["config"].clone();
    let lan = config["networks"][0]["id"].clone();
    config["vpn"]["tailscale"] = json!({
        "enabled": true, "routes": [lan], "exit_node": false, "hostname": null, "auth_key": null,
    });
    let save = json!({ "base": head["revision"]["id"], "config": config, "secrets": {} });
    let (status, saved) = send(&demo, Method::PUT, "/v1/site", Some(&save)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (status, body) = send(&demo, Method::POST, "/v1/vpn/tailscale/login", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["detail"].as_str().unwrap().contains("observe mode"),
        "{body}"
    );
    let (status, body) = send(&demo, Method::POST, "/v1/vpn/tailscale/logout", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}
