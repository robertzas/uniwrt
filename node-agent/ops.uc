// The agent's operations (`proto::ops`). Each answers once, through `done(result)` or
// `done(null, code, msg)`: at once, or later for a command run in a task of the event loop.
// The streams (subscriptions, log following, ticks) need the event loop; without it they
// answer `unsupported` and the controller polls instead.

import * as fs from "fs";
import * as bus from "./bus.uc";
import { ban } from "./ban.uc";
import { bss } from "./bss.uc";
import * as exec from "./exec.uc";
import * as logs from "./logs.uc";
import * as streams from "./streams.uc";
import * as ticks from "./ticks.uc";
import * as sets from "./collect.uc";
import { BAD_REQUEST, NOT_FOUND, UNSUPPORTED, FAILED } from "./proto.uc";
import { now_ms } from "./util.uc";

const DEFAULT_MAX_BYTES = 1048576;
const MAX_BYTES = 16777216;
// How long after its timeout a command's task may take to report before it counts as lost.
const TASK_GRACE_MS = 5000;
const SCAN_DEFAULT_TIMEOUT_MS = 20000;
const SCAN_MAX_TIMEOUT_MS = 60000;
// How long the scan's process may take beyond the scan (the surveys and the results).
const SCAN_GRACE_MS = 5000;
const SCAN_MAX_OUTPUT = 4194304;
// The agent's directory, where scan.uc is.
const DIR = sourcepath(0, true);

function ping(args, done) {
	done({ ts: now_ms() });
}

function collect(args, done) {
	let names = args.sets ?? keys(sets.SETS);
	if (type(names) != "array")
		return done(null, BAD_REQUEST, "sets must be an array");
	done(sets.run(names));
}

function ubus(args, done) {
	if (type(args.path) != "string" || type(args.method) != "string")
		return done(null, BAD_REQUEST, "path and method must be strings");
	if (args.data != null && type(args.data) != "object")
		return done(null, BAD_REQUEST, "data must be an object");
	let r = bus.call(args.path, args.method, args.data);
	if (r.code)
		return done(null, r.code, r.msg);
	done(r.ok);
}

function reply(done, r) {
	if (r?.ok)
		return done(r.ok);
	if (r?.err)
		return done(null, r.err[0], r.err[1]);
	done(null, FAILED, "the command ended without a result");
}

// Runs a parsed command and hands `finish` exec.run's result. Under the event loop the command
// runs in a forked task, so other requests go on meanwhile.
function command(req, done, loop, finish) {
	if (!loop)
		return finish(exec.run(req));
	let guard = null;
	let task = loop.task(() => exec.run(req), (r) => {
		guard?.cancel();
		finish(r);
	});
	guard = loop.timer(req.timeout_ms + TASK_GRACE_MS, () => {
		task.kill();
		done(null, FAILED, "the command's task ended without a result");
	});
}

function run(args, done, loop) {
	let req = exec.parse(args);
	if (req.err)
		return done(null, BAD_REQUEST, req.err);
	command(req, done, loop, (r) => reply(done, r));
}

// The answer scan.uc printed.
function scanned(done, r) {
	if (r?.err)
		return done(null, r.err[0], r.err[1]);
	let out = null;
	try {
		out = json(r.ok.stdout);
	}
	catch (e) {
		// Reported below, with what the worker wrote to stderr.
	}
	if (out?.ok)
		return done(out.ok);
	if (type(out?.err) == "array")
		return done(null, out.err[0], out.err[1]);
	done(null, FAILED, `the scan ended without a result: ${trim(r.ok?.stderr ?? "")}`);
}

// `scan`: scan.uc in a process of its own (it explains why), as a command.
function scan(args, done, loop) {
	if (type(args.ifname) != "string" || !match(args.ifname, /^[A-Za-z0-9._-]{1,15}$/))
		return done(null, BAD_REQUEST, "ifname must be an interface name");
	let freqs = args.freqs ?? [];
	if (type(freqs) != "array" || length(filter(freqs, (f) => type(f) != "int" || f <= 0)))
		return done(null, BAD_REQUEST, "freqs must be an array of frequencies in MHz");
	let timeout_ms = args.timeout_ms ?? SCAN_DEFAULT_TIMEOUT_MS;
	if (type(timeout_ms) != "int" || timeout_ms < 1000 || timeout_ms > SCAN_MAX_TIMEOUT_MS)
		return done(null, BAD_REQUEST, `timeout_ms must be between 1000 and ${SCAN_MAX_TIMEOUT_MS}`);
	let worker = { ifname: args.ifname, freqs: freqs, timeout_ms: timeout_ms };
	let req = {
		argv: ["ucode", `${DIR}/scan.uc`, sprintf("%J", worker)],
		timeout_ms: timeout_ms + SCAN_GRACE_MS,
		max_output: SCAN_MAX_OUTPUT,
	};
	command(req, done, loop, (r) => scanned(done, r));
}

function read_file(args, done) {
	if (type(args.path) != "string" || !length(args.path))
		return done(null, BAD_REQUEST, "path must be a non-empty string");
	let max_bytes = args.max_bytes ?? DEFAULT_MAX_BYTES;
	if (type(max_bytes) != "int" || max_bytes < 0 || max_bytes > MAX_BYTES)
		return done(null, BAD_REQUEST, `max_bytes must be between 0 and ${MAX_BYTES}`);
	let data = fs.readfile(args.path, max_bytes + 1);
	if (data == null) {
		let err = fs.error();
		return done(null, index(err, "No such file") == 0 ? NOT_FOUND : FAILED, `${args.path}: ${err}`);
	}
	let truncated = length(data) > max_bytes;
	if (truncated)
		data = substr(data, 0, max_bytes);
	done({ data: args.base64 ? b64enc(data) : data, truncated: truncated });
}

// An op that needs the event loop.
function streaming(op) {
	return (args, done, loop) => loop ? op(args, done, loop) :
		done(null, UNSUPPORTED, "the agent serves requests one at a time, without an event loop");
}

const OPS = {
	ping: ping, collect: collect, ubus: ubus, exec: run, read_file: read_file, scan: scan, bss: bss,
	ban: ban,
	subscribe: streaming(streams.subscribe), unsubscribe: streaming(streams.unsubscribe),
	log_follow: streaming(logs.log_follow), set_interval: streaming(ticks.set_interval),
};

// Ends what would outlive the agent: a log stream's logread.
function shutdown() {
	logs.stop();
}

export { OPS, shutdown };
