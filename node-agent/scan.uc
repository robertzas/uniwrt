// The `scan` op's worker, run by ops.uc as a process of its own (`ucode scan.uc '<args>'`) so that
// its nl80211 socket is its own: a forked task would share the agent's, and the agent's ticks
// could read the scan's replies. Scans one radio through `ifname` as an access point may
// (nl80211's AP flag; the cache is flushed so only this scan's results come back), waits for the
// end of the scan, and prints one JSON line: { ok: { started_ms, ended_ms, bss, survey_before,
// survey_after } } or { err: [code, msg] }. Each result's information elements are put back
// together as bytes, in base64; the controller parses them (crates/node/src/facts/ies.rs).

import { nl80211, request } from "./wifi.uc";
import { now_ms } from "./util.uc";
import { NOT_FOUND, TIMEOUT, UNSUPPORTED, FAILED } from "./proto.uc";

function survey(ifname) {
	let found = [];
	for (let e in request("NL80211_CMD_GET_SURVEY", "NLM_F_DUMP", { dev: ifname })) {
		let s = e?.survey_info;
		if (s?.frequency)
			push(found, { frequency: s.frequency, noise: s.noise, time: s.time, busy: s.busy });
	}
	return found;
}

function ies(list) {
	if (type(list) != "array")
		return null;
	let bytes = "";
	for (let ie in list)
		bytes += chr(ie.type, length(ie.data)) + ie.data;
	return b64enc(bytes);
}

// The code a failed nl80211 request maps to.
function failed(what, err) {
	if (match(err, /not supported/i))
		return [UNSUPPORTED, `${what}: ${err}`];
	if (match(err, /no such device/i))
		return [NOT_FOUND, `${what}: ${err}`];
	if (match(err, /busy/i))
		return [FAILED, `busy: ${what}: ${err}`];
	return [FAILED, `${what}: ${err}`];
}

function scan(args) {
	let nl = nl80211(), c = nl.const, ifname = args.ifname;
	// Joining the scan group first: the end of the scan cannot come before we listen.
	nl.waitfor([c.NL80211_CMD_NEW_SCAN_RESULTS], 1);
	let before = survey(ifname);
	let started = now_ms();
	let msg = { dev: ifname, scan_flags: c.NL80211_SCAN_FLAG_AP | c.NL80211_SCAN_FLAG_FLUSH };
	if (length(args.freqs))
		msg.scan_frequencies = args.freqs;
	if (nl.request(c.NL80211_CMD_TRIGGER_SCAN, 0, msg) == null && nl.error())
		return { err: failed("the radio cannot scan", nl.error()) };
	let deadline = started + args.timeout_ms;
	for (;;) {
		let left = deadline - now_ms();
		if (left <= 0) {
			nl.request(c.NL80211_CMD_ABORT_SCAN, 0, { dev: ifname });
			return { err: [TIMEOUT, `the scan did not end within ${args.timeout_ms} ms`] };
		}
		let ev = nl.waitfor([c.NL80211_CMD_NEW_SCAN_RESULTS, c.NL80211_CMD_SCAN_ABORTED], left);
		// Another interface's scan (a radio scans through one interface at a time).
		if (!ev || (ev.msg?.dev != null && ev.msg.dev != ifname))
			continue;
		if (ev.cmd == c.NL80211_CMD_SCAN_ABORTED)
			return { err: [FAILED, "the scan was aborted"] };
		break;
	}
	let ended = now_ms();
	let bss = [];
	for (let e in request("NL80211_CMD_GET_SCAN", "NLM_F_DUMP", { dev: ifname })) {
		let b = e?.bss;
		if (b?.bssid && b.frequency)
			push(bss, {
				bssid: b.bssid,
				frequency: b.frequency,
				signal_mbm: b.signal_mbm,
				capability: b.capability,
				seen_ms_ago: b.seen_ms_ago,
				ies: ies(b.information_elements ?? b.beacon_ies),
			});
	}
	return {
		ok: {
			started_ms: started, ended_ms: ended, bss: bss,
			survey_before: before, survey_after: survey(ifname),
		},
	};
}

let result;
try {
	result = scan(json(ARGV[0]));
}
catch (e) {
	result = { err: [match(e.message, /not installed/) ? UNSUPPORTED : FAILED, `${e.message}`] };
}
printf("%J\n", result);
