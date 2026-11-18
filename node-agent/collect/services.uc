// The `services` set: the gateway's network services as they run (D-132): each DDNS name's
// registered address and last update from ddns-scripts' run directory (`<section>.ip`, and
// `.update` in seconds of uptime), whether its updater runs (`.pid`), and how many
// https-dns-proxy instances procd runs, with the clock and uptime that convert `.update`.

import * as fs from "fs";
import * as uci from "uci";
import * as bus from "../bus.uc";
import { read } from "../util.uc";

const RUNDIR = "/var/run/ddns";

// The running instances of an init script's service, or null where procd has none.
function instances(name) {
	let r = bus.call("service", "list", { name: name });
	let service = r.code ? null : r.ok?.[name];
	if (service == null)
		return null;
	let n = 0;
	for (let id, instance in service.instances ?? {})
		if (instance.running)
			n++;
	return n;
}

function collect() {
	let cursor = uci.cursor();
	let rundir = cursor.get("ddns", "global", "ddns_rundir") ?? RUNDIR;
	let ddns = [];
	cursor.foreach("ddns", "service", (s) => {
		let base = `${rundir}/${s[".name"]}`;
		let pid = int(read(`${base}.pid`) ?? "0");
		push(ddns, {
			id: s[".name"],
			ip: read(`${base}.ip`),
			update: read(`${base}.update`),
			running: pid > 0 && fs.access(`/proc/${pid}`) == true,
		});
	});
	let uptime = split(read("/proc/uptime") ?? "", " ")[0];
	return {
		now: time(),
		uptime: uptime ? int(uptime) : null,
		ddns: ddns,
		doh: instances("https-dns-proxy"),
	};
}

export { collect };
