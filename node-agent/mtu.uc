// The `mtu` op (features/mesh.md, Wired backhaul; D-094): whether a cable carries packets of
// `size` bytes, batman-adv's 1532 on a backhaul's VLAN device (`<port>.<vid>`). It pings IPv6's
// all-nodes group on `dev` from the device's link-local address, with echo requests whose IPv6
// packet is `size` bytes long, and answers the device's MTU and the link-local addresses that
// answered at that size (the node's own left out). A device whose MTU is below `size` is not
// pinged: the kernel would fragment the requests, and they would pass.
// A passing probe shows the peer receives `size` from this end; its reply may come back
// fragmented, so the controller probes from both ends. BusyBox's ping does it (any build with
// IPv6 has it), in a command of its own: it takes a few seconds.

import * as fs from "fs";
import { BAD_REQUEST, NOT_FOUND, FAILED } from "./proto.uc";

// IPv6's minimum MTU, up to a jumbo frame.
const MIN_SIZE = 1280;
const MAX_SIZE = 9216;
// The IPv6 and ICMPv6 headers: ping's payload is the size less these.
const HEADERS = 48;
const COUNT = "3";
const DEADLINE_S = "3";
const TIMEOUT_MS = 6000;

// An address as eight groups of four hex digits, the way /proc/net/if_inet6 writes it; null
// when it isn't IPv6.
function expand(addr) {
	let halves = split(addr, "::");
	if (length(halves) > 2)
		return null;
	let head = length(halves[0]) ? split(halves[0], ":") : [];
	let tail = length(halves) == 2 && length(halves[1]) ? split(halves[1], ":") : [];
	let missing = 8 - length(head) - length(tail);
	if (length(halves) == 1 ? missing != 0 : missing < 1)
		return null;
	let groups = [...head];
	for (let i = 0; i < missing; i++)
		push(groups, "0");
	push(groups, ...tail);
	let out = "";
	for (let g in groups) {
		if (!match(g, /^[0-9a-fA-F]{1,4}$/))
			return null;
		out += sprintf("%04s", lc(g));
	}
	return replace(out, " ", "0");
}

// The device's own addresses, expanded.
function own(dev) {
	let out = {};
	for (let line in split(fs.readfile("/proc/net/if_inet6") ?? "", "\n")) {
		let f = split(trim(line), /\s+/);
		if (length(f) == 6 && f[5] == dev)
			out[f[0]] = true;
	}
	return out;
}

// Checks the request: returns { dev, size, mtu, req } (req null when the MTU is too small to
// ping), or { err: [code, msg] }.
function request(args) {
	if (type(args.dev) != "string" || !match(args.dev, /^[A-Za-z0-9._-]{1,15}$/))
		return { err: [BAD_REQUEST, "dev must be an interface name"] };
	if (type(args.size) != "int" || args.size < MIN_SIZE || args.size > MAX_SIZE)
		return { err: [BAD_REQUEST, `size must be between ${MIN_SIZE} and ${MAX_SIZE}`] };
	let mtu = fs.readfile(`/sys/class/net/${args.dev}/mtu`);
	if (mtu == null)
		return { err: [NOT_FOUND, `no device ${args.dev}`] };
	mtu = int(trim(mtu));
	let probe = { dev: args.dev, size: args.size, mtu: mtu, req: null };
	if (mtu >= args.size)
		probe.req = {
			argv: ["ping", "-6", "-c", COUNT, "-w", DEADLINE_S, "-s", sprintf("%d", args.size - HEADERS),
				"-I", args.dev, "ff02::1"],
			timeout_ms: TIMEOUT_MS,
			max_output: 65536,
		};
	return probe;
}

// The answer to a probe, from ping's result (null when it wasn't pinged).
function answer(probe, r) {
	let peers = [];
	if (probe.req) {
		if (r?.err)
			return { err: r.err };
		if (r.ok.code == null || r.ok.code > 1)
			return { err: [FAILED, `ping failed: ${trim(r.ok.stderr)}`] };
		let self = own(probe.dev), seen = {};
		// BusyBox writes "<n> bytes from <addr>: seq=…"; <n> counts ICMPv6's header and payload.
		let bytes = `${probe.size - 40}`;
		for (let line in split(r.ok.stdout, "\n")) {
			let m = match(line, /^([0-9]+) bytes from ([0-9a-fA-F:]+)(%[^:]*)?: /);
			let addr = m && m[1] == bytes ? expand(m[2]) : null;
			if (addr && !self[addr] && !seen[addr]) {
				seen[addr] = true;
				push(peers, lc(m[2]));
			}
		}
	}
	return { ok: { dev: probe.dev, size: probe.size, mtu: probe.mtu, peers: peers } };
}

export { request, answer };
