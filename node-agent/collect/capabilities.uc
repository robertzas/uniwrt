// The `capabilities` set: features detected on the node, never inferred from its version: the
// package manager, optional software, what hostapd and wpa_supplicant were built with, and
// how the switch is driven.

import * as fs from "fs";
import { status } from "../exec.uc";
import { readjson } from "../util.uc";

// Package managers, by the path of their binary.
const PACKAGE_MANAGERS = { apk: "/usr/bin/apk", opkg: "/bin/opkg" };

// Optional software, by package name (`facts::Software`), and the paths that reveal it.
const SOFTWARE = {
	"batman-adv": ["/sys/module/batman_adv", "/lib/modules/*/batman-adv.ko"],
	"batctl": ["/usr/sbin/batctl"],
	"iw": ["/usr/sbin/iw"],
	"rpcd-mod-iwinfo": ["/usr/lib/rpcd/iwinfo.so"],
	"usteer": ["/usr/sbin/usteer"],
	"nlbwmon": ["/usr/sbin/nlbwmon"],
	"lldpd": ["/usr/sbin/lldpd"],
	"iperf3": ["/usr/bin/iperf3"],
	"luci": ["/www/luci-static"],
};

// Build features that OpenWrt's hostapd and wpa_supplicant report through `-v<feature>`: exit
// status 0 means built in.
const HOSTAPD = ["eap", "11r", "acs", "sae", "owe", "wps"];
const SUPPLICANT = ["eap", "sae", "owe", "mesh"];

function features(binary, names) {
	if (!fs.access(binary, "x"))
		return null;
	let found = [];
	for (let name in names)
		if (status([binary, `-v${name}`], 2000) == 0)
			push(found, name);
	return found;
}

function collect() {
	let package_managers = [];
	for (let name, path in PACKAGE_MANAGERS)
		if (fs.access(path, "x"))
			push(package_managers, name);
	let software = [];
	for (let name, paths in SOFTWARE)
		for (let pattern in paths)
			if (length(fs.glob(pattern) ?? [])) {
				push(software, name);
				break;
			}
	return {
		package_managers: package_managers,
		software: software,
		hostapd: features("/usr/sbin/hostapd", HOSTAPD),
		wpa_supplicant: features("/usr/sbin/wpa_supplicant", SUPPLICANT),
		swconfig: readjson("/etc/board.json")?.switch != null,
		dsa: length(fs.glob("/sys/class/net/*/dsa") ?? []) > 0,
	};
}

export { collect };
