//! The `tailscale` op on real OpenWrt (D-140; TESTING.md, Agent): through the agent, a stock
//! node (no package) lists the op and answers `not_found`; and `tailscale.uc` itself, run by
//! ucode against a fake `tailscale` CLI (a shell script standing in for the package, which the
//! containers lack): `set` hands the CLI the prefs with DNS and routes refused and netfilter
//! off; a login with a key passes it as a file of root's that is gone afterwards, and reports
//! the CLI's last words on stderr when refused; a login without one starts the CLI in the
//! background and answers the URL the daemon hands out, then the same URL while it waits; a
//! logged-in daemon gets no login; bad requests are refused before anything runs. Ignored by
//! default: `just test-agent` runs it. The real CLI was checked by hand on 24.10.8 and 25.12.6
//! containers (REFERENCE.md).

#![allow(clippy::unwrap_used)]

mod openwrt;

use std::time::Duration;

use openwrt::{adopt, link, run};
use serde_json::{Value, json};
use uniwrt_model::site::vpn::tailscale::Prefs;
use uniwrt_node::{
    AgentLaunch, Bundle, Error, NodeLink as _,
    proto::ops::{Tailscale, TailscaleAction},
};

const LONG: Duration = Duration::from_secs(20);
const SCRIPT_PATH: &str = "/tmp/uniwrt-tailscale-test.uc";

/// The fake CLI: logs each call's argv (and a key file's mode and content) to `calls`, answers
/// `status --json` from `state`, and plays tailscaled's part in a login.
const CLI: &str = r#"#!/bin/sh
dir=/tmp/uniwrt-ts
echo "$*" >>"$dir/calls"
case "$1" in
status) cat "$dir/state" ;;
set) echo "Warning: netfilter=off; configure iptables yourself." ;;
login)
	for arg; do
		case "$arg" in --auth-key=file:*)
			key=${arg#--auth-key=file:}
			ls -l "$key" | cut -c1-10 >>"$dir/calls"
			cat "$key" >>"$dir/calls"; echo >>"$dir/calls"
			case "$(cat "$key")" in *bad*)
				echo "To authenticate, visit:"
				echo "backend error: invalid key: unable to validate API key" >&2
				exit 1 ;;
			esac
			echo '{"BackendState": "Running", "AuthURL": ""}' >"$dir/state"
			exit 0 ;;
		esac
	done
	sleep 1
	echo '{"BackendState": "NeedsLogin", "AuthURL": "https://login.tailscale.com/a/test1"}' >"$dir/state"
	sleep 30 ;;
esac
"#;

/// Runs the ops in turn and prints what each answered, with the CLI's calls.
const SCRIPT: &str = r#"
import * as fs from "fs";
import { tailscale, paths } from "/tmp/uniwrt/agent/tailscale.uc";

const dir = "/tmp/uniwrt-ts";
const key = dir + "/key";
// The agent makes it when it starts; this script runs without it.
fs.mkdir("/tmp/uniwrt/run", 0o700);
paths(dir + "/tailscale", key);
let state = (s) => fs.writefile(dir + "/state", sprintf("%J", { BackendState: s, AuthURL: "" }));
let answer = (args) => {
	let out = null;
	tailscale(args, (r, c, m) => out = r ?? { code: c, msg: m }, null);
	return out;
};
let prefs = { routes: ["10.0.30.0/24", "10.0.40.0/24"], exit_node: true, hostname: "home-gw" };
let out = {};
state("NeedsLogin");
out.bad_route = answer({ action: "set", prefs: { ...prefs, routes: ["10.0.0.0/24;reboot"] } });
out.bad_key = answer({ action: "set", prefs: prefs, auth_key: "tskey-x" });
out.set = answer({ action: "set", prefs: prefs });
out.refused = answer({ action: "login", prefs: prefs, auth_key: "tskey-auth-bad1" });
out.key_gone = fs.access(key) != true;
out.login = answer({ action: "login", prefs: prefs });
out.again = answer({ action: "login", prefs: prefs });
out.keyed = answer({ action: "login", prefs: prefs, auth_key: "tskey-auth-good1" });
out.running = answer({ action: "login", prefs: prefs });
out.down = answer({ action: "down" });
out.calls = split(trim(fs.readfile(dir + "/calls")), "\n");
print(sprintf("%J\n", out));
"#;

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn a_stock_node_lists_the_op_and_lacks_the_package() {
    let (_session, node) = adopt().await;
    let (link, hello) = link(&node, AgentLaunch::Bundle).await;
    assert!(hello.caps.ops.iter().any(|op| op == "tailscale"));
    let prefs = Prefs {
        routes: Vec::new(),
        exit_node: false,
        hostname: String::new(),
    };
    let request = Tailscale::new(TailscaleAction::Set, prefs);
    match link.request(&request, LONG).await {
        Err(Error::Remote { code, .. }) => assert_eq!(code, "not_found"),
        other => panic!("not refused: {other:?}"),
    }
    link.shutdown().await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_op_drives_the_cli_with_the_prefs_and_keeps_the_key_out_of_argv() {
    let (session, _node) = adopt().await;
    Bundle::get().ensure(&session).await.unwrap();
    let write = "rm -rf /tmp/uniwrt-ts && mkdir /tmp/uniwrt-ts && printf %s \"$1\" \
        >/tmp/uniwrt-ts/tailscale && chmod 755 /tmp/uniwrt-ts/tailscale && printf %s \"$2\" >\"$3\"";
    run(
        &session,
        &["sh", "-c", write, "sh", CLI, SCRIPT, SCRIPT_PATH],
    )
    .await;
    let out: Value = serde_json::from_str(&run(&session, &["ucode", SCRIPT_PATH]).await).unwrap();
    run(&session, &["rm", "-rf", SCRIPT_PATH, "/tmp/uniwrt-ts"]).await;

    assert_eq!(out["bad_route"]["code"], "bad_request");
    assert_eq!(out["bad_key"]["code"], "bad_request");
    assert_eq!(out["set"], json!({"state": "NeedsLogin", "auth_url": null}));
    // The CLI's stderr, not the URL it printed first.
    assert_eq!(
        out["refused"],
        json!({"code": "failed", "msg": "backend error: invalid key: unable to validate API key"})
    );
    assert_eq!(out["key_gone"], true);
    let url = json!("https://login.tailscale.com/a/test1");
    assert_eq!(
        out["login"],
        json!({"state": "NeedsLogin", "auth_url": url})
    );
    assert_eq!(
        out["again"],
        json!({"state": "NeedsLogin", "auth_url": url})
    );
    assert_eq!(out["keyed"], json!({"state": "Running", "auth_url": null}));
    // Logged in: no URL, no login.
    assert_eq!(
        out["running"],
        json!({"state": "Running", "auth_url": null})
    );
    assert_eq!(out["down"], json!({"state": "Running", "auth_url": null}));

    let calls: Vec<&str> = out["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .filter(|c| !c.starts_with("status"))
        .collect();
    let flags = "--advertise-routes=10.0.30.0/24,10.0.40.0/24 --advertise-exit-node=true \
        --hostname=home-gw --accept-dns=false --accept-routes=false --netfilter-mode=off";
    assert_eq!(calls[0], format!("set {flags}"));
    assert_eq!(
        calls[1],
        format!("login --auth-key=file:/tmp/uniwrt-ts/key --timeout=30s {flags}")
    );
    // Root's alone, the key in the file only.
    assert_eq!(calls[2], "-rw-------");
    assert_eq!(calls[3], "tskey-auth-bad1");
    assert_eq!(calls[4], format!("login --timeout=60s {flags}"));
    assert!(calls[5].starts_with("login --auth-key=file:"));
    assert_eq!(calls[8], "down");
    assert!(calls.iter().all(|c| !c.contains("--auth-key=tskey")));
}
