// The `btm` op (features/wifi.md, Steering & balancing: controller-sent transition requests;
// D-091, D-093): asks one client to move, with an 802.11v BSS transition request from the access
// point it is connected to (hostapd's `bss_transition_request` on `hostapd.<ifname>`). The
// request carries the MBO fields, which 24.10's and 25.12's hostapd need when MBO is built in
// (the full wpad): without them it answers "Invalid argument" and sends nothing.
// `neighbors` are the candidates, best first, as neighbor report elements in hex (the controller
// adds each one's preference); with candidates the request is abridged (BSSes not listed are
// not preferred). `imminent` tells the client it will be disconnected: with `kick_ms`, hostapd
// does so after that long unless it left, and `reassoc_s` asks an MBO client not to come back to
// this access point for that long. The client's answer comes as hostapd's
// `bss-transition-response` notification (the `hostapd` topic).

import * as bus from "./bus.uc";
import { BAD_REQUEST, NOT_FOUND, UNSUPPORTED, FAILED } from "./proto.uc";

const MAX_NEIGHBORS = 16;
// A neighbor report element's body is at most 255 bytes.
const MAX_NR = 510;
// hostapd's MBO transition reasons, 0 (unspecified) to 9 (premium AP).
const MAX_REASON = 9;
const MAX_KICK_MS = 60000;
// hostapd takes the reassociation delay through a u8 on 24.10 and 25.12.
const MAX_REASSOC_S = 255;
// MBO's cellular data preference: 1, "don't use", the default on OpenWrt's main branch. hostapd
// writes the attribute whatever the client is; it only matters to phones.
const CELL_PREF = 1;
// How long the request stays valid, in beacon intervals (usteer's value).
const VALIDITY = 100;

// The request's dialog token, 1 to 255: the answer echoes it.
let token = 0;

function int_in(v, max) {
	return v == null || (type(v) == "int" && v >= 0 && v <= max);
}

function valid(a) {
	return type(a.addr) == "string" && match(a.addr, /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/) &&
		(a.neighbors == null || (type(a.neighbors) == "array" &&
			length(a.neighbors) <= MAX_NEIGHBORS &&
			!length(filter(a.neighbors, (n) => type(n) != "string" || length(n) > MAX_NR ||
				!match(n, /^([0-9a-f]{2}){13,}$/))))) &&
		int_in(a.reason, MAX_REASON) && (a.imminent == null || type(a.imminent) == "bool") &&
		int_in(a.kick_ms, MAX_KICK_MS) && int_in(a.reassoc_s, MAX_REASSOC_S) &&
		(a.imminent || (!a.kick_ms && !a.reassoc_s));
}

// The access point `addr` is connected to: { path, ifname, client } or null.
function serving(conn, addr) {
	for (let path in sort(conn.list() ?? [])) {
		if (!match(path, /^hostapd\.[^.]+$/))
			continue;
		let r = bus.call(path, "get_clients");
		let client = r.ok?.clients?.[addr];
		if (client)
			return { path: path, ifname: substr(path, 8), client: client };
	}
	return null;
}

function btm(args, done) {
	if (!valid(args))
		return done(null, BAD_REQUEST,
			`btm takes addr (lower-case MAC), neighbors? (up to ${MAX_NEIGHBORS} reports in hex), ` +
			`reason? (0 to ${MAX_REASON}), imminent?, and with it kick_ms? (to ${MAX_KICK_MS}) ` +
			`and reassoc_s? (to ${MAX_REASSOC_S})`);
	let c = bus.connection();
	if (c.code)
		return done(null, c.code, c.msg);
	let at = serving(c.ok, args.addr);
	if (!at)
		return done(null, NOT_FOUND, `${args.addr} isn't connected to an access point here`);
	// hostapd counts the disconnect timer in beacon intervals (TU, 1.024 ms).
	let timer = 0;
	if (args.kick_ms) {
		let interval = bus.call(at.path, "get_status").ok?.beacon_interval;
		if (type(interval) != "int" || interval <= 0)
			interval = 100;
		timer = min(65535, max(1, int((args.kick_ms * 1000 + interval * 1024 - 1) / (interval * 1024))));
	}
	token = token % 255 + 1;
	let neighbors = args.neighbors ?? [];
	let request = {
		addr: args.addr,
		dialog_token: token,
		validity_period: VALIDITY,
		abridged: length(neighbors) > 0,
		neighbors: neighbors,
		disassociation_imminent: args.imminent ?? false,
		disassociation_timer: timer,
		mbo_reason: args.reason ?? 0,
		cell_pref: CELL_PREF,
		reassoc_delay: args.reassoc_s ?? 0,
	};
	let r = bus.call(at.path, "bss_transition_request", request);
	if (r.code == NOT_FOUND && index(r.msg, "Method not found") == 0)
		return done(null, UNSUPPORTED, `${at.ifname}: hostapd has no 802.11v (the basic wpad)`);
	if (r.code)
		return done(null, r.code == NOT_FOUND ? NOT_FOUND : FAILED, `${at.ifname}: ${r.msg}`);
	done({
		ifname: at.ifname, token: token, timer: timer,
		mbo: at.client.mbo == true,
	});
}

export { btm };
