// The `rrm` op (features/wifi.md, Roaming): 802.11k neighbor reports. It answers every BSS's own
// report (hostapd's `rrm_nr_get_own`: BSSID, SSID and the report element in hex), which only
// BSSes with 802.11k have, and gives the BSSes named in `lists` their neighbors through
// `rrm_nr_set`, which replaces a BSS's list. hostapd keeps the list in memory (a BSS that
// restarts forgets it), so the controller sends the lists again and again: a BSS whose list
// (`rrm_nr_list`, its own report left out) already holds the same entries is left alone.

import * as bus from "./bus.uc";
import { BAD_REQUEST, FAILED } from "./proto.uc";

const MAX_BSSES = 64;
const MAX_NEIGHBORS = 64;
// A neighbor report element's body is at most 255 bytes.
const MAX_NR = 510;

function valid_entry(e) {
	return type(e) == "array" && length(e) == 3 && type(e[0]) == "string" &&
		match(e[0], /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/) && type(e[1]) == "string" &&
		length(e[1]) <= 32 && type(e[2]) == "string" && length(e[2]) <= MAX_NR &&
		match(e[2], /^([0-9a-f]{2})+$/);
}

function valid(lists) {
	if (type(lists) != "object" || length(lists) > MAX_BSSES)
		return false;
	for (let ifname, list in lists)
		if (!match(ifname, /^[A-Za-z0-9._-]{1,15}$/) || type(list) != "array" ||
		    length(list) > MAX_NEIGHBORS || length(filter(list, (e) => !valid_entry(e))))
			return false;
	return true;
}

// A list's entries as sorted keys, the way hostapd's and the controller's lists compare.
function keyed(list, own) {
	let out = [];
	for (let e in list ?? [])
		if (type(e) == "array" && length(e) == 3 && lc(e[0]) != own)
			push(out, join("|", [lc(e[0]), e[1], lc(e[2])]));
	return join(",", sort(out));
}

function rrm(args, done) {
	let lists = args.lists ?? {};
	if (!valid(lists))
		return done(null, BAD_REQUEST,
			`lists must map up to ${MAX_BSSES} interfaces to up to ${MAX_NEIGHBORS} [bssid (lower-case MAC), ssid, report (hex)]`);
	let c = bus.connection();
	if (c.code)
		return done(null, c.code, c.msg);
	let own = [], set = [], failed = [], seen = {};
	for (let path in sort(c.ok.list() ?? [])) {
		if (index(path, "hostapd.") != 0)
			continue;
		let ifname = substr(path, 8);
		let r = bus.call(path, "rrm_nr_get_own");
		let v = r.ok?.value;
		// Without 802.11k (or gone meanwhile) a BSS has no report.
		if (r.code || type(v) != "array" || length(v) != 3)
			continue;
		let bssid = lc(v[0]);
		push(own, { ifname: ifname, bssid: bssid, ssid: v[1], nr: lc(v[2]) });
		seen[ifname] = true;
		if (!(ifname in lists))
			continue;
		let held = bus.call(path, "rrm_nr_list");
		if (!held.code && keyed(held.ok?.list, bssid) == keyed(lists[ifname], bssid))
			continue;
		let s = bus.call(path, "rrm_nr_set", { list: lists[ifname] });
		if (s.code)
			push(failed, { ifname: ifname, error: s.msg });
		else
			push(set, ifname);
	}
	for (let ifname in lists)
		if (!seen[ifname])
			push(failed, { ifname: ifname, error: "no access point with 802.11k there" });
	if (length(failed) && !length(own))
		return done(null, FAILED, failed[0].error);
	done({ own: own, set: set, failed: failed });
}

export { rrm };
