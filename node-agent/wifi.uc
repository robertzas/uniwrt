// Shared by the Wi-Fi sets: the kernel phys from sysfs, and nl80211 (ucode-mod-nl80211, which
// hostapd depends on), loaded on first use because images without Wi-Fi lack it.

import * as fs from "fs";
import { read } from "./util.uc";

let nl = null;

// The kernel phys: [{ name, index, device }]. `device` is where the phy's `device` link leads, from
// which OpenWrt derives a radio's `path`.
function phys() {
	let found = [];
	for (let name in fs.lsdir("/sys/class/ieee80211") ?? []) {
		let dir = `/sys/class/ieee80211/${name}`;
		push(found, { name: name, index: int(read(`${dir}/index`)), device: fs.realpath(`${dir}/device`) });
	}
	return found;
}

// An nl80211 request by the names of its command and flags: its replies, [] when there are none.
// An error raises, failing the set that made the request.
function request(cmd, flags, msg) {
	if (!nl) {
		try {
			nl = require("nl80211");
		}
		catch (e) {
			die("ucode-mod-nl80211 is not installed");
		}
	}
	let reply = nl.request(nl.const[cmd], flags ? nl.const[flags] : 0, msg);
	if (reply == null) {
		let err = nl.error();
		if (err)
			die(`nl80211 ${cmd}: ${err}`);
		return [];
	}
	return type(reply) == "array" ? reply : [reply];
}

// The network interfaces of every phy; P2P devices, which have none, are left out.
function interfaces() {
	return filter(request("NL80211_CMD_GET_INTERFACE", "NLM_F_DUMP"), (i) => i?.ifname != null);
}

export { phys, request, interfaces };
