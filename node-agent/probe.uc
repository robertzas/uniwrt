// The WAN quality monitor's round (D-144), run on the gateway over exec rather than by the
// agent, which a slow round must not hold up: `ucode /var/run/uniwrt/agent/probe.uc <spec>`, the
// spec one JSON argument:
//   {"count":10,"icmp":[{"key":"first-hop","host":null},{"key":"cloudflare","host":"1.1.1.1"}],
//    "dns":[{"key":"resolver","server":null,"name":"openwrt.org"},
//           {"key":"isp-dns","server":"upstreams","name":"openwrt.org"}],
//    "http":[{"key":"web","url":"http://connectivitycheck.gstatic.com/generate_204"}]}
// A null ICMP host is the default route's gateway (the ISP's first hop); a null DNS server is
// the gateway's own resolver, and "upstreams" each server the WAN learned, a result each keyed
// `<key>@<address>`. Prints one JSON object:
//   {"route":true,"first_hop":"10.0.0.1","results":[{"key":…,"sent":10,"rtt_ms":[…],"error":null}]}
// The pings run at once (BusyBox's ping, one a second), each in the background of one fixed
// script; the lookups and fetches run after, one at a time, each timed. Values reach commands
// as argv, never as script text.

import * as fs from "fs";
import { RUN, run } from "./exec.uc";
import { mono_ms } from "./util.uc";

// $1 pings per host, $2 the directory whose `hosts` names them, one a line; each host's output
// goes to $2/<line number>. A watchdog ends the pings a name lookup holds up.
const PINGS = 'n=$1; d=$2; i=0; p=; while read -r h; do i=$((i+1)); ping -c "$n" -W 1 -w "$((n+2))" "$h" >"$d/$i" 2>&1 & p="$p $!"; done <"$d/hosts"; (sleep "$((n+6))"; kill $p 2>/dev/null) & w=$!; wait $p; kill "$w" 2>/dev/null; exit 0';
const RESOLV = ["/tmp/resolv.conf.d/resolv.conf.auto", "/tmp/resolv.conf.auto"];
const DNS_TIMEOUT_MS = 3000;
const HTTP_TIMEOUT_S = 5;
const RTF_REJECT = 0x0200;

// The IPv4 default route's gateway, from /proc/net/route (little-endian hex).
function routes() {
	let route = false, first_hop = null;
	for (let line in split(fs.readfile("/proc/net/route") ?? "", "\n")) {
		let f = split(trim(line), /\s+/);
		if (length(f) < 8 || f[1] != "00000000" || f[7] != "00000000")
			continue;
		route = true;
		let g = f[2];
		if (g != "00000000" && !first_hop)
			first_hop = sprintf("%d.%d.%d.%d", hex(substr(g, 6, 2)), hex(substr(g, 4, 2)),
				hex(substr(g, 2, 2)), hex(substr(g, 0, 2)));
	}
	// An IPv6 default route that isn't a reject (DS-Lite, IPv6-only lines).
	for (let line in split(fs.readfile("/proc/net/ipv6_route") ?? "", "\n")) {
		let f = split(trim(line), /\s+/);
		if (length(f) >= 10 && f[0] == "00000000000000000000000000000000" && f[1] == "00" &&
		    !(hex(f[8]) & RTF_REJECT) && f[9] != "lo")
			route = true;
	}
	return { route: route, first_hop: first_hop };
}

function first_line(text) {
	for (let line in split(text ?? "", "\n"))
		if (length(trim(line)))
			return trim(line);
	return null;
}

function ping_result(key, count, out) {
	let rtt = [], sent = count, error = null;
	for (let line in split(out ?? "", "\n")) {
		let m = match(line, /time=([0-9.]+) ms/);
		if (m) {
			push(rtt, +m[1]);
			continue;
		}
		m = match(line, /^([0-9]+) packets transmitted/);
		if (m)
			sent = int(m[1]);
		else if (match(line, /^ping: /) && !error)
			error = trim(substr(line, 6));
	}
	if (!length(rtt) && !error)
		error = "no answer";
	return { key: key, sent: sent, rtt_ms: rtt, error: length(rtt) ? null : error };
}

// Starts the pings in the background (the lookups and fetches run meanwhile); `finish` waits
// for them and reads what they found. Only constants and the agent's own path reach the shell.
function start_pings(spec, hop) {
	let count = spec.count ?? 10, keys = [], hosts = [], early = [];
	for (let target in spec.icmp ?? []) {
		let host = target.host ?? hop;
		if (host == null || match(host, /[[:space:]'"\\]/)) {
			push(early, { key: target.key, sent: 0, rtt_ms: [], error: host == null ? "no first hop" : "bad host" });
			continue;
		}
		push(hosts, host);
		push(keys, target.key);
	}
	if (!length(hosts))
		return { early: early, keys: [] };
	let dir = sprintf("%s/probe.%s", RUN, fs.readlink("/proc/self"));
	fs.mkdir(dir, 0o700);
	fs.writefile(`${dir}/hosts`, join("\n", hosts) + "\n");
	let pipe = fs.popen(sprintf("/bin/sh -c '%s' sh %d '%s' </dev/null >/dev/null 2>&1", PINGS, count, dir), "r");
	return { early: early, keys: keys, dir: dir, count: count, pipe: pipe };
}

function finish_pings(started) {
	let results = started.early;
	if (!length(started.keys))
		return results;
	if (started.pipe)
		started.pipe.close();
	for (let i = 0; i < length(started.keys); i++) {
		let path = sprintf("%s/%d", started.dir, i + 1);
		let out = fs.readfile(path, 65536);
		let result = ping_result(started.keys[i], started.count, out);
		if (!length(trim(out ?? "")))
			result.error = started.pipe ? "ping did not finish" : "cannot run ping";
		push(results, result);
		fs.unlink(path);
	}
	fs.unlink(`${started.dir}/hosts`);
	fs.rmdir(started.dir);
	return results;
}

function upstreams() {
	for (let path in RESOLV) {
		let text = fs.readfile(path, 65536);
		if (text == null)
			continue;
		let servers = [];
		for (let line in split(text, "\n")) {
			let m = match(trim(line), /^nameserver\s+(\S+)$/);
			if (m && index(servers, m[1]) < 0)
				push(servers, m[1]);
		}
		return servers;
	}
	return [];
}

function timed(key, argv, timeout_ms, answered) {
	let start = mono_ms();
	let done = run({ argv: argv, timeout_ms: timeout_ms, max_output: 65536 });
	let ms = mono_ms() - start;
	if (done.err)
		return { key: key, sent: 1, rtt_ms: [], error: done.err[1] };
	let out = done.ok.stdout + done.ok.stderr;
	if (answered(done.ok, out))
		return { key: key, sent: 1, rtt_ms: [ms], error: null };
	return { key: key, sent: 1, rtt_ms: [], error: first_line(out) ?? sprintf("exit %d", done.ok.code ?? -1) };
}

// An answer, even NXDOMAIN, is the server working; a timeout or a refusal isn't.
function looked_up(ok, out) {
	return !match(out, /timed out|no servers could be reached|refused|SERVFAIL/) &&
		match(out, /Name:|Address:|NXDOMAIN|can't find|NODATA/) != null;
}

function lookups(spec) {
	let results = [];
	for (let target in spec.dns ?? []) {
		let servers = target.server == "upstreams" ? upstreams() : [target.server ?? "127.0.0.1"];
		if (target.server == "upstreams" && !length(servers))
			push(results, { key: target.key, sent: 0, rtt_ms: [], error: "no upstream servers" });
		for (let server in servers) {
			let key = target.server == "upstreams" ? `${target.key}@${server}` : target.key;
			push(results, timed(key, ["nslookup", target.name, server], DNS_TIMEOUT_MS, looked_up));
		}
	}
	return results;
}

function fetches(spec) {
	let results = [];
	for (let target in spec.http ?? []) {
		let argv = ["uclient-fetch", "-q", "-O", "/dev/null", "-T", sprintf("%d", HTTP_TIMEOUT_S), "--", target.url];
		push(results, timed(target.key, argv, (HTTP_TIMEOUT_S + 2) * 1000, (ok) => ok.code == 0));
	}
	return results;
}

let spec;
try {
	spec = json(ARGV[0] ?? "");
}
catch (e) {
	spec = null;
}
if (type(spec) != "object") {
	warn("usage: probe.uc <spec as JSON>\n");
	exit(2);
}
fs.mkdir("/var/run/uniwrt", 0o700);
fs.mkdir(RUN, 0o700);
let out = routes();
let started = start_pings(spec, out.first_hop);
let others = [...lookups(spec), ...fetches(spec)];
out.results = [...finish_pings(started), ...others];
printf("%J\n", out);
