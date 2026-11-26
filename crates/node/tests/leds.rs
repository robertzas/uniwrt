//! The `leds` op on real OpenWrt (D-138; TESTING.md, Agent): through the agent, the `system`
//! set reports the LEDs and `on` without a saved state changes nothing (a container's
//! `/sys/class/leds` is the host's, read-only, so `off` isn't sent there); and `leds.uc`
//! itself, run by ucode over a tree of plain files standing for `/sys/class/leds`: `off` saves
//! each LED's trigger, brightness and settings (a subdirectory's too) and darkens it, an LED lit
//! again is saved anew, and `on` puts them back, the trigger first, and forgets them. Ignored
//! by default: `just test-agent` runs it against its container.

#![allow(clippy::unwrap_used)]

mod openwrt;

use std::time::Duration;

use openwrt::{adopt, link, run};
use serde_json::{Value, json};
use uniwrt_model::facts::NodeFacts;
use uniwrt_node::{
    AgentLaunch, Bundle, NodeLink as _, facts,
    proto::ops::{Collect, Leds, Set},
};

const LONG: Duration = Duration::from_secs(20);
const SCRIPT_PATH: &str = "/var/run/uniwrt-leds-test.uc";

/// Builds the tree, then runs `off`, relights one LED, `off` again, resets the settings as a
/// trigger change would, and `on`; prints what each step left.
const SCRIPT: &str = r#"
import * as fs from "fs";
import { leds, state, paths } from "/var/run/uniwrt/agent/leds.uc";

const root = "/var/run/uniwrt-leds";
const saved = "/var/run/uniwrt-leds.json";
system(`rm -rf ${root} ${saved}`);
for (let dir in [root, `${root}/wan`, `${root}/power`, `${root}/usb`, `${root}/usb/ports`])
	fs.mkdir(dir);
let put = (path, text) => fs.writefile(`${root}/${path}`, text);
let get = (path) => trim(fs.readfile(`${root}/${path}`) ?? "");
put("wan/trigger", "none [netdev] timer");
put("wan/brightness", "0");
put("wan/max_brightness", "255");
put("wan/device_name", "wan");
put("wan/link", "1");
put("power/trigger", "[none] timer");
put("power/brightness", "255");
put("usb/trigger", "none [usbport]");
put("usb/brightness", "0");
put("usb/ports/usb1-port1", "1");
paths(root, saved);

let answer = (r, code, msg) => r ?? { code: code, msg: msg };
let out = { before: state() };
leds({ off: true }, (r, c, m) => out.off = answer(r, c, m));
out.dark = map(["wan", "power", "usb"], (led) => [get(`${led}/trigger`), get(`${led}/brightness`)]);
out.saved = json(fs.readfile(saved));

put("power/brightness", "128");
leds({ off: true }, (r, c, m) => out.again = answer(r, c, m));
out.power = json(fs.readfile(saved)).leds.power;

put("wan/device_name", "");
put("usb/ports/usb1-port1", "0");
leds({ off: false }, (r, c, m) => out.on = answer(r, c, m));
out.back = {
	wan: [get("wan/trigger"), get("wan/device_name"), get("wan/link")],
	power: [get("power/trigger"), get("power/brightness")],
	usb: [get("usb/trigger"), get("usb/ports/usb1-port1")],
};
out.forgotten = fs.access(saved) != true;
system(`rm -rf ${root}`);
print(sprintf("%J\n", out));
"#;

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn the_agent_reports_its_leds_and_turns_them_on_without_a_saved_state() {
    let (_session, node) = adopt().await;
    let (link, hello) = link(&node, AgentLaunch::Bundle).await;
    assert!(hello.caps.ops.iter().any(|op| op == "leds"));
    let sets = Collect {
        sets: vec![Set::System],
    };
    let mut found = NodeFacts::default();
    let _ = facts::apply(&link.request(&sets, LONG).await.unwrap(), &mut found);
    let leds = found.system.leds.expect("the system set reports the LEDs");
    assert!(!leds.held, "{leds:?}");
    let on = link.request(&Leds { off: false }, LONG).await.unwrap();
    assert_eq!(on, leds);
    link.shutdown().await;
}

#[tokio::test]
#[ignore = "needs an OpenWrt container booted with procd: just test-agent"]
async fn leds_go_dark_and_come_back_as_they_were() {
    let (session, _node) = adopt().await;
    Bundle::get().ensure(&session).await.unwrap();
    run(
        &session,
        &[
            "sh",
            "-c",
            "printf %s \"$1\" >\"$2\"",
            "sh",
            SCRIPT,
            SCRIPT_PATH,
        ],
    )
    .await;
    let out: Value = serde_json::from_str(&run(&session, &["ucode", SCRIPT_PATH]).await).unwrap();
    run(&session, &["rm", "-f", SCRIPT_PATH]).await;

    assert_eq!(out["before"], json!({"count": 3, "lit": 3, "held": false}));
    assert_eq!(out["off"], json!({"count": 3, "lit": 0, "held": true}));
    assert_eq!(
        out["dark"],
        json!([["none", "0"], ["none", "0"], ["none", "0"]])
    );
    let saved = &out["saved"]["leds"];
    assert_eq!(
        saved["wan"],
        json!({"trigger": "netdev", "brightness": 0,
               "settings": {"device_name": "wan", "link": "1"}})
    );
    assert_eq!(
        saved["power"],
        json!({"trigger": "none", "brightness": 255, "settings": {}})
    );
    assert_eq!(saved["usb"]["settings"], json!({"ports/usb1-port1": "1"}));
    // Lit again before the second `off`: its newer state is the one kept.
    assert_eq!(out["again"]["held"], json!(true));
    assert_eq!(out["power"]["brightness"], json!(128));

    assert_eq!(out["on"], json!({"count": 3, "lit": 3, "held": false}));
    assert_eq!(
        out["back"],
        json!({"wan": ["netdev", "wan", "1"], "power": ["none", "128"],
               "usb": ["usbport", "1"]})
    );
    assert_eq!(out["forgotten"], json!(true));
}
