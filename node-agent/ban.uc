// The `ban` op (features/wifi.md, Client actions; D-084): keeps clients off every access point
// of the node, or lets them back, through hostapd's `del_client` on each BSS (`hostapd.<ifname>`).
// A ban of `ms` milliseconds deauthenticates the client where it is connected and has hostapd
// refuse its probes, authentications and associations until it lapses; hostapd keeps bans in
// memory, so a BSS that restarts forgets them and the controller bans again. `ms` 0 lifts a ban;
// `del_client` would deauthenticate a client connected there, so a lift skips the BSSes it is on,
// unless `kick` asks for that (a kick without a ban).
// hostapd's timer takes the ban in microseconds in an int, so bans stay well under 35 minutes.

import * as bus from "./bus.uc";
import { BAD_REQUEST, FAILED } from "./proto.uc";

const MAX_MS = 1800000;
const MAX_BANS = 512;
// IEEE 802.11 reason 5: disassociated because the AP is unable to handle all associated STAs.
const REASON = 5;

function valid(b) {
	return type(b) == "object" && type(b.addr) == "string" &&
		match(b.addr, /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/) && type(b.ms) == "int" &&
		b.ms >= 0 && b.ms <= MAX_MS && (b.kick == null || type(b.kick) == "bool");
}

function ban(args, done) {
	let bans = args.bans;
	if (type(bans) != "array" || length(bans) > MAX_BANS ||
	    length(filter(bans, (b) => !valid(b))))
		return done(null, BAD_REQUEST,
			`bans must be up to ${MAX_BANS} of { addr (lower-case MAC), ms (0 to ${MAX_MS}), kick? }`);
	let c = bus.connection();
	if (c.code)
		return done(null, c.code, c.msg);
	let bsses = [], kicked = [], failed = [];
	for (let path in sort(c.ok.list() ?? [])) {
		if (index(path, "hostapd.") != 0)
			continue;
		let ifname = substr(path, 8);
		let clients = bus.call(path, "get_clients");
		// A BSS that went away meanwhile has no clients to keep off.
		if (clients.code)
			continue;
		let here = clients.ok?.clients ?? {};
		push(bsses, ifname);
		for (let b in bans) {
			let on = b.addr in here;
			if (!b.ms && !b.kick && on)
				continue;
			let r = bus.call(path, "del_client",
				{ addr: b.addr, reason: REASON, deauth: true, ban_time: b.ms });
			if (r.code)
				push(failed, { ifname: ifname, addr: b.addr, error: r.msg });
			else if (on)
				push(kicked, { ifname: ifname, addr: b.addr });
		}
	}
	if (length(failed) && !length(bsses))
		return done(null, FAILED, failed[0].error);
	done({ bsses: bsses, kicked: kicked, failed: failed });
}

export { ban };
