// The `services` set: the gateway's network services as they run (D-132): each DDNS name's
// registered address and last update from ddns-scripts' run directory (`<section>.ip`, and
// `.update` in seconds of uptime), whether its updater runs (`.pid`), and how many
// https-dns-proxy instances procd runs, with the clock and uptime that convert `.update`; and
// the DNS blocklist the controller pushed (D-133): the header line of its servers-file (only the
// first bytes are read: the list may hold a million lines) and whether its conf file is there;
// and the WireGuard tunnel `vpn` (D-140) as `wg show vpn dump` reports it, without its keys;
// and Tailscale (D-140) as `tailscale status --json` reports it, without its login URL.

import * as fs from "fs";
import * as uci from "uci";
import * as bus from "../bus.uc";
import { read } from "../util.uc";

const RUNDIR = "/var/run/ddns";
const BLOCKLIST = "/tmp/dnsmasq.uniwrt.d/blocklist.servers";
const BLOCKLIST_CONF = "/tmp/dnsmasq.uniwrt.d/blocklist.conf";
const WG = "/usr/bin/wg";
// netifd names the tunnel's device after its interface (render's `interface vpn`). The command is
// a constant: no value goes into the shell. It is read through a pipe rather than `exec`, whose
// output files would hold the private and preshared keys the dump starts each line with; `wg`
// reads the kernel over netlink, so it doesn't wait on the network.
const WG_DUMP = "/usr/bin/wg show vpn dump 2>/dev/null";
const TAILSCALE = "/usr/sbin/tailscale";
// tailscaled's default socket: the package's init script names none.
const TS_SOCKET = "/var/run/tailscale/tailscaled.sock";
// A constant command, answered by the local daemon without waiting on the network.
const TS_STATUS = "/usr/sbin/tailscale status --json 2>/dev/null";
// The most peers sent (model's TAILSCALE_MAX_PEERS), online ones first.
const TS_PEERS = 200;

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

// The tunnel: the interface's line (private key, public key, port, fwmark) gives its public key
// and port; each peer's line (public key, preshared key, endpoint, allowed IPs, latest handshake
// in seconds since 1970 or 0, rx and tx bytes, keepalive) its public key, endpoint, handshake and
// traffic. The keys are dropped as the lines are read. Null without the device or `wg`.
function wireguard() {
	if (!fs.access("/sys/class/net/vpn") || !fs.access(WG, "x"))
		return null;
	let p = fs.popen(WG_DUMP, "r");
	if (!p)
		return null;
	let tunnel = null;
	for (let line = p.read("line"); length(line); line = p.read("line")) {
		let f = split(rtrim(line, "\n"), "\t");
		if (tunnel == null) {
			if (length(f) < 4)
				break;
			tunnel = { public_key: f[1], port: f[2], peers: [] };
		}
		else if (length(f) >= 8)
			push(tunnel.peers, { public_key: f[0], endpoint: f[2], handshake: f[4], rx: f[5], tx: f[6] });
	}
	p.close();
	return tunnel;
}

// Tailscale: the daemon's state, the gateway's names, addresses and routes, its warnings and its
// peers, with only the fields the controller reads; never the login URL (`AuthURL`), which the
// facts would show to every user. Null without the package; `state` null where the daemon
// doesn't answer.
function tailscale() {
	if (!fs.access(TAILSCALE, "x"))
		return null;
	if (!fs.access(TS_SOCKET))
		return { state: null };
	let p = fs.popen(TS_STATUS, "r");
	if (!p)
		return { state: null };
	let text = p.read("all");
	p.close();
	let s = null;
	try {
		s = json(text);
	}
	catch (e) {
		return { state: null };
	}
	if (type(s) != "object")
		return { state: null };
	let me = s.Self ?? {};
	let peers = [];
	let count = 0;
	for (let online in [true, false])
		for (let key, peer in s.Peer ?? {}) {
			if ((peer.Online == true) != online)
				continue;
			count++;
			if (length(peers) < TS_PEERS)
				push(peers, {
					name: peer.HostName, dns_name: peer.DNSName, os: peer.OS, ips: peer.TailscaleIPs,
					online: peer.Online, active: peer.Active, exit_node_option: peer.ExitNodeOption,
					cur_addr: peer.CurAddr, relay: peer.Relay, last_seen: peer.LastSeen,
					rx: peer.RxBytes, tx: peer.TxBytes,
				});
		}
	return {
		state: s.BackendState,
		version: s.Version,
		tailnet: s.CurrentTailnet?.Name,
		dns_suffix: s.CurrentTailnet?.MagicDNSSuffix ?? s.MagicDNSSuffix,
		self: {
			dns_name: me.DNSName, ips: me.TailscaleIPs, online: me.Online,
			allowed_ips: me.AllowedIPs, primary_routes: me.PrimaryRoutes,
		},
		health: s.Health,
		peer_count: count,
		peers: peers,
	};
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
	let head = fs.readfile(BLOCKLIST, 128);
	return {
		now: time(),
		uptime: uptime ? int(uptime) : null,
		ddns: ddns,
		doh: instances("https-dns-proxy"),
		blocklist: head == null ? null : split(head, "\n")[0],
		blocklist_conf: fs.access(BLOCKLIST_CONF) == true,
		wireguard: wireguard(),
		tailscale: tailscale(),
	};
}

export { collect };
