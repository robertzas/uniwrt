// The `batman` set: each batman-adv mesh device as batctl's JSON queries report it: the mesh, its
// hard interfaces, originators, neighbors, gateways and both translation tables. A query that
// fails is null.

import * as fs from "fs";
import * as bus from "../bus.uc";
import * as exec from "../exec.uc";
import { read } from "../util.uc";

const BATCTL = "/usr/sbin/batctl";
const QUERIES = { mesh: "mj", hardifs: "hj", originators: "oj", neighbors: "nj", gateways: "gwj", global: "tgj",
	local: "tlj" };

// The devices of interfaces that netifd runs with proto batadv, which names the mesh device after
// the interface.
function configured() {
	let found = [];
	for (let iface in bus.get("network.interface", "dump")?.interface ?? [])
		if (iface.proto == "batadv")
			push(found, iface.l3_device ?? iface.interface);
	return found;
}

// Devices that may be meshes made outside netifd: batman-adv sets no DEVTYPE and its hard
// interfaces are its lower devices. Macvlan and team devices look alike; `mj` tells them apart.
function unconfigured(known) {
	let found = [];
	for (let name in fs.lsdir("/sys/class/net") ?? []) {
		let dir = `/sys/class/net/${name}`;
		if (index(known, name) < 0 && length(fs.glob(`${dir}/lower_*`) ?? []) &&
		    index(read(`${dir}/uevent`) ?? "", "DEVTYPE=") < 0)
			push(found, name);
	}
	return found;
}

function query(dev, name) {
	let r = exec.run({ argv: [BATCTL, "meshif", dev, name], timeout_ms: 5000, max_output: 4194304 });
	if (r.ok?.code != 0)
		return null;
	try {
		return json(r.ok.stdout);
	}
	catch (e) {
		return null;
	}
}

// A device whose `mj` query fails is not a mesh.
function collect() {
	let known = configured(), result = {};
	if (!fs.access(BATCTL, "x")) {
		if (length(known))
			die("batctl is not installed");
		return result;
	}
	for (let dev in [...known, ...unconfigured(known)]) {
		let mesh = { mesh: query(dev, QUERIES.mesh) };
		if (mesh.mesh == null)
			continue;
		for (let key, name in QUERIES)
			if (key != "mesh")
				mesh[key] = query(dev, name);
		result[dev] = mesh;
	}
	return result;
}

export { collect };
