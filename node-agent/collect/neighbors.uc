// The `neighbors` set: the IPv4 and IPv6 neighbor caches and the bridges' forwarding databases.
// With ucode-mod-rtnl (which hostapd depends on, so on images with Wi-Fi) both come from
// rtnetlink, keeping unicast neighbors that have a link-layer address and the addresses bridges
// learned. Without it, IPv4 neighbors come from /proc/net/arp and each bridge's database from its
// binary `brforward`, in base64, with its ports' numbers.

import * as fs from "fs";
import { read, pick } from "../util.uc";

// linux/neighbour.h, linux/rtnetlink.h
const NUD_INCOMPLETE = 0x01;
const NUD_FAILED = 0x20;
const NUD_NOARP = 0x40;
const NUD_PERMANENT = 0x80;
const RTN_UNICAST = 1;
const AF_INET = 2;
const AF_INET6 = 10;
const NEIGH = ["family", "dev", "dst", "lladdr", "state", "flags"];
const FDB = ["dev", "master", "lladdr", "vlan"];

function dump(rtnl, msg) {
	let reply = rtnl.request(rtnl.const.RTM_GETNEIGH, rtnl.const.NLM_F_DUMP, msg);
	if (reply == null && rtnl.error())
		die(`rtnetlink: ${rtnl.error()}`);
	return reply ?? [];
}

function from_rtnl(rtnl) {
	let neigh = [], fdb = [];
	for (let e in dump(rtnl, {}))
		if ((e.family == AF_INET || e.family == AF_INET6) && e.type == RTN_UNICAST && e.lladdr &&
		    !(e.state & (NUD_INCOMPLETE | NUD_FAILED | NUD_NOARP)))
			push(neigh, pick(e, NEIGH));
	for (let e in dump(rtnl, { family: rtnl.const.AF_BRIDGE }))
		if (e.master && e.lladdr && !(e.state & NUD_PERMANENT))
			push(fdb, pick(e, FDB));
	return { neigh: neigh, fdb: fdb };
}

function from_proc() {
	let bridges = {};
	for (let name in fs.lsdir("/sys/class/net") ?? []) {
		let dir = `/sys/class/net/${name}`;
		let data = fs.readfile(`${dir}/brforward`);
		if (data == null)
			continue;
		let ports = {};
		for (let port in fs.lsdir(`${dir}/brif`) ?? [])
			ports[port] = read(`${dir}/brif/${port}/port_no`);
		bridges[name] = { brforward: b64enc(data), ports: ports };
	}
	return { arp: fs.readfile("/proc/net/arp", 1048576), bridges: bridges };
}

function collect() {
	let rtnl = null;
	try {
		rtnl = require("rtnl");
	}
	catch (e) {
		return { proc: from_proc() };
	}
	return { rtnl: from_rtnl(rtnl) };
}

export { collect };
