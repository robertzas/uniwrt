// The `tailscale` op (features/network.md, VPN; D-140): the gateway's Tailscale daemon as the
// controller wants it, through the `tailscale` CLI (its prefs live in tailscaled's state file, not
// UCI). Every action but `logout` and `down` hands the CLI the controller's prefs: the subnet
// routes, the exit node offer and the machine name, with the tailnet's DNS and routes refused
// (the gateway's dnsmasq and routes stay its own) and `--netfilter-mode=off` (fw4 is the only
// firewall: tailscaled's own input rule drops 100.64.0.0/10 from every other interface, which
// would cut a CGNAT WAN off). `tailscale login` resets the prefs it isn't given, so it always
// gets them all.
//
// - `set`: the prefs (`tailscale set`), logged in or not.
// - `up`: running again after `down` (`tailscale up --reset` with the prefs).
// - `down`: stopped, still logged in. `logout`: logged out; the prefs stay.
// - `login` with `auth_key`: logs in by itself. The key goes to the CLI as a file of root's
//   (`--auth-key=file:`), never in argv, and is removed once the CLI ends.
// - `login` without: starts an interactive login in the background and answers the login URL
//   tailscaled hands out (`status --json`'s `AuthURL`, which it keeps while the login waits),
//   or the URL already waiting. A machine that is logged in gets no URL.
//
// Answers `{ state, auth_url }`: tailscaled's state afterwards and, for `login`, the URL.

import * as fs from "fs";
import * as exec from "./exec.uc";
import { BAD_REQUEST, NOT_FOUND, FAILED } from "./proto.uc";

// Swapped by the agent tests for a fake CLI (`paths`).
let cli = "/usr/sbin/tailscale";
let key_path = "/var/run/uniwrt/tailscale.key";

// How long `set`, `down` and `logout` (which tells the coordination server) may take.
const COMMAND_MS = 30000;
// How long a login with a key, or `up`, waits for tailscaled to run (the CLI's `--timeout`).
const RUNNING_S = 30;
// How long an interactive login waits for its URL, and how often it looks.
const URL_MS = 15000;
const URL_POLL_MS = 250;
// The background CLI's own `--timeout`: tailscaled keeps the URL after the CLI gives up.
const BACKGROUND_S = 60;
// The most a task may take, with the CLI's own time.
const TASK_MS = (RUNNING_S + 15) * 1000;

// Lengths are checked apart: musl compiles a bounded repetition as that many copies, and
// `{1,250}` alone took 21 MB at every agent start (D-149).
const ROUTE = /^([0-9]{1,3}\.){3}[0-9]{1,3}\/[0-9]{1,2}$/;
const HOSTNAME = /^([A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?)?$/;
const HOSTNAME_MAX = 63;
const AUTH_KEY = /^tskey-[A-Za-z0-9_-]+$/;
const AUTH_KEY_MAX = 256;
const ACTIONS = { set: true, up: true, down: true, logout: true, login: true };

function paths(binary, key) {
	cli = binary;
	key_path = key;
}

// The request checked: returns it, or { err: message }.
function parse(args) {
	if (!ACTIONS[args.action])
		return { err: "action must be set, up, down, logout or login" };
	let req = { action: args.action };
	if (args.action == "down" || args.action == "logout")
		return req;
	let p = args.prefs;
	if (type(p) != "object")
		return { err: "prefs must be an object" };
	if (type(p.routes) != "array" || length(p.routes) > 64 ||
	    length(filter(p.routes, (r) => type(r) != "string" || !match(r, ROUTE))))
		return { err: "prefs.routes must be up to 64 IPv4 prefixes" };
	if (type(p.exit_node) != "bool")
		return { err: "prefs.exit_node must be a boolean" };
	if (type(p.hostname) != "string" || length(p.hostname) > HOSTNAME_MAX || !match(p.hostname, HOSTNAME))
		return { err: "prefs.hostname must be one DNS label, or empty" };
	req.prefs = p;
	if (args.auth_key != null) {
		if (args.action != "login" || type(args.auth_key) != "string" ||
		    length(args.auth_key) > AUTH_KEY_MAX || !match(args.auth_key, AUTH_KEY))
			return { err: "auth_key must be a Tailscale auth key, for login" };
		req.auth_key = args.auth_key;
	}
	return req;
}

// The CLI's flags for the prefs. Values are argv elements of their own flags: no shell.
function flags(p) {
	return [
		"--advertise-routes=" + join(",", p.routes),
		"--advertise-exit-node=" + (p.exit_node ? "true" : "false"),
		"--hostname=" + p.hostname,
		"--accept-dns=false",
		"--accept-routes=false",
		"--netfilter-mode=off",
	];
}

// tailscaled's state and waiting login URL, read through a pipe (the URL never reaches a file);
// null where it doesn't answer.
function status() {
	let p = fs.popen(`${cli} status --json 2>/dev/null`, "r");
	if (!p)
		return null;
	let text = p.read("all");
	p.close();
	let s = null;
	try {
		s = json(text);
	}
	catch (e) {
		return null;
	}
	if (type(s?.BackendState) != "string")
		return null;
	return { state: s.BackendState, auth_url: length(s.AuthURL ?? "") ? s.AuthURL : null };
}

// tailscaled's state, and the URL a login got.
function answer(auth_url) {
	let s = status();
	if (s == null)
		return { err: [FAILED, "tailscaled doesn't answer"] };
	return { ok: { state: s.state, auth_url: auth_url } };
}

// What the CLI said when it failed: the last line of its stderr (its warnings go to stdout, and
// `up` prints a URL before its timeout), else of its stdout.
function said(out) {
	for (let text in [out.stderr, out.stdout]) {
		let lines = filter(split(trim(text ?? ""), "\n"), (l) => length(trim(l)));
		if (length(lines))
			return trim(lines[-1]);
	}
	return `tailscale exited with ${out.code ?? "a signal"}`;
}

// Runs the CLI with `args`; returns { ok: answer } or { err: [code, msg] }.
function run(args, timeout_ms) {
	let r = exec.run({ argv: [cli, ...args], timeout_ms: timeout_ms, max_output: 65536 });
	if (r.err)
		return r;
	if (r.ok.code !== 0)
		return { err: [FAILED, said(r.ok)] };
	return answer(null);
}

function with_key(req) {
	let f = fs.open(key_path, "w", 0600);
	if (!f)
		return { err: [FAILED, `cannot write the auth key: ${fs.error()}`] };
	f.write(req.auth_key);
	f.close();
	let r = run(["login", "--auth-key=file:" + key_path, `--timeout=${RUNNING_S}s`, ...flags(req.prefs)],
		(RUNNING_S + 10) * 1000);
	fs.unlink(key_path);
	return r;
}

function interactive(req) {
	let s = status();
	if (s == null)
		return { err: [FAILED, "tailscaled doesn't answer"] };
	if (s.state != "NeedsLogin" || s.auth_url)
		return { ok: s };
	// In the background, its output dropped: the URL is read from tailscaled.
	let argv = [cli, "login", `--timeout=${BACKGROUND_S}s`, ...flags(req.prefs)];
	if (system(["/bin/sh", "-c", '"$@" </dev/null >/dev/null 2>&1 &', "sh", ...argv], 5000) !== 0)
		return { err: [FAILED, "cannot start tailscale login"] };
	for (let waited = 0; waited < URL_MS; waited += URL_POLL_MS) {
		sleep(URL_POLL_MS);
		s = status();
		if (s && (s.auth_url || s.state != "NeedsLogin"))
			return { ok: s };
	}
	return { err: [FAILED, `tailscaled handed out no login URL within ${URL_MS / 1000} s`] };
}

// The action, blocking.
function work(req) {
	switch (req.action) {
	case "set":
		return run(["set", ...flags(req.prefs)], COMMAND_MS);
	case "up":
		return run(["up", "--reset", `--timeout=${RUNNING_S}s`, ...flags(req.prefs)], (RUNNING_S + 10) * 1000);
	case "down":
		return run(["down"], COMMAND_MS);
	case "logout":
		return run(["logout"], COMMAND_MS);
	case "login":
		return req.auth_key ? with_key(req) : interactive(req);
	}
}

function tailscale(args, done, loop) {
	let req = parse(args);
	if (req.err)
		return done(null, BAD_REQUEST, req.err);
	if (!fs.access(cli, "x"))
		return done(null, NOT_FOUND, "tailscale isn't installed");
	let finish = (r) => r?.ok ? done(r.ok) :
		done(null, r?.err?.[0] ?? FAILED, r?.err?.[1] ?? "the tailscale op ended without a result");
	if (!loop)
		return finish(work(req));
	let guard = null;
	let task = loop.task(() => work(req), (r) => {
		guard?.cancel();
		finish(r);
	});
	guard = loop.timer(TASK_MS, () => {
		task.kill();
		fs.unlink(key_path);
		done(null, FAILED, "the tailscale op's task ended without a result");
	});
}

export { tailscale, paths };
