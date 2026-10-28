// The `devices` set: every kernel network device, from sysfs, with the counters of
// /proc/net/dev. Sysfs sees devices that netifd does not manage, such as Wi-Fi interfaces
// outside a bridge.

import * as fs from "fs";
import { read } from "../util.uc";

// Attributes read as text; unreadable ones (`carrier` and `speed` of a device that is down) are
// null.
const ATTRS = ["type", "address", "mtu", "flags", "carrier", "speed", "duplex", "operstate", "uevent"];

function collect() {
	let devices = {};
	for (let name in fs.lsdir("/sys/class/net") ?? []) {
		let dir = `/sys/class/net/${name}`;
		// Skips files such as `bonding_masters`.
		if (fs.stat(dir)?.type != "directory")
			continue;
		let dev = {};
		for (let attr in ATTRS)
			dev[attr] = read(`${dir}/${attr}`);
		let master = fs.readlink(`${dir}/master`);
		dev.master = master ? fs.basename(master) : null;
		dev.wireless = fs.access(`${dir}/phy80211`) == true;
		dev.physical = fs.access(`${dir}/device`) == true;
		devices[name] = dev;
	}
	return { sys: devices, net_dev: fs.readfile("/proc/net/dev", 1048576) };
}

export { collect };
