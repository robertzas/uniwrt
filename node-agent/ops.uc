// The agent's operations (`proto::ops`). Each answers once, through `done(result)` or
// `done(null, code, msg)`: at once, or later for a command run in a task of the event loop.

import * as fs from "fs";
import * as bus from "./bus.uc";
import * as exec from "./exec.uc";
import { SETS } from "./collect.uc";
import { BAD_REQUEST, NOT_FOUND, FAILED } from "./proto.uc";
import { now_ms } from "./util.uc";

const DEFAULT_MAX_BYTES = 1048576;
const MAX_BYTES = 16777216;
// How long after its timeout a command's task may take to report before it counts as lost.
const TASK_GRACE_MS = 5000;

function ping(args, done) {
	done({ ts: now_ms() });
}

// Sets that fail report an error each; the others are still collected.
function collect(args, done) {
	let names = args.sets ?? keys(SETS);
	if (type(names) != "array")
		return done(null, BAD_REQUEST, "sets must be an array");
	let result = { sets: {}, errors: {} };
	for (let name in names) {
		let set = SETS[name];
		if (type(set) != "function") {
			result.errors[name] = { code: BAD_REQUEST, msg: `unknown set ${name}` };
			continue;
		}
		try {
			result.sets[name] = set();
		}
		catch (e) {
			result.errors[name] = { code: FAILED, msg: e.message };
		}
	}
	done(result);
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

// Under the event loop the command runs in a forked task, so other requests go on meanwhile.
function run(args, done, loop) {
	let req = exec.parse(args);
	if (req.err)
		return done(null, BAD_REQUEST, req.err);
	if (!loop)
		return reply(done, exec.run(req));
	let guard = null;
	let task = loop.task(() => exec.run(req), (r) => {
		guard?.cancel();
		reply(done, r);
	});
	guard = loop.timer(req.timeout_ms + TASK_GRACE_MS, () => {
		task.kill();
		done(null, FAILED, "the command's task ended without a result");
	});
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

const OPS = { ping: ping, collect: collect, ubus: ubus, exec: run, read_file: read_file };

export { OPS };
