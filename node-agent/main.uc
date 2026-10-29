// The uniwrt node agent: serves the controller's requests from stdin and answers on stdout, one
// JSON object per line (proto.uc), until stdin closes. The controller installs it under
// /tmp/uniwrt/agent and runs `ucode /tmp/uniwrt/agent/main.uc` (ARCHITECTURE.md, Node protocol).
//
// With ucode-mod-uloop, a reader task feeds requests into an event loop and each command runs in
// a task of its own, so a slow command holds up nothing else, and the agent pushes events and
// ticks (streams.uc, logs.uc, ticks.uc). Without it (images without Wi-Fi before 25.12),
// requests are served one at a time and nothing is pushed. `--sync` forces that mode, for tests.
// Nothing but the protocol may reach stdout; diagnostics go to stderr, which the controller
// keeps for its error messages.

import * as fs from "fs";
import { OPS, shutdown } from "./ops.uc";
import { SETS } from "./collect.uc";
import { TOPICS } from "./streams.uc";
import { RUN } from "./exec.uc";
import { PROTO, BAD_REQUEST, FAILED, send, answer, failure } from "./proto.uc";
import { read, shvars } from "./util.uc";

// How often the event loop checks that its reader task is still alive.
const WATCH_MS = 5000;

function warn(msg) {
	fs.stderr.write(`${msg}\n`);
	fs.stderr.flush();
}

// The event loop module, or null for the synchronous mode.
function event_loop() {
	if (index(ARGV, "--sync") >= 0)
		return null;
	try {
		let uloop = require("uloop");
		return type(uloop.task) == "function" ? uloop : null;
	}
	catch (e) {
		return null;
	}
}

function serve(line, loop) {
	let request;
	try {
		request = json(trim(line));
	}
	catch (e) {
		return warn(`ignored a malformed request: ${e.message}`);
	}
	let id = type(request) == "object" ? request.id : null;
	if (type(id) != "int")
		return warn("ignored a request without an integer id");
	let answered = false;
	let done = (result, code, msg) => {
		if (answered)
			return;
		answered = true;
		send(code ? failure(id, code, msg) : answer(id, result));
	};
	let op = OPS[request.op];
	if (type(op) != "function")
		return done(null, BAD_REQUEST, `unknown op ${request.op}`);
	let args = request.args ?? {};
	if (type(args) != "object")
		return done(null, BAD_REQUEST, "args must be an object");
	try {
		op(args, done, loop);
	}
	catch (e) {
		done(null, FAILED, e.message);
	}
}

function serve_sync() {
	for (let line = fs.stdin.read("line"); length(line); line = fs.stdin.read("line"))
		serve(line, null);
}

// A forked task reads stdin with blocking reads and hands each line to the loop: the loop
// itself must not read stdin, because buffered reads on a non-blocking descriptor lose data.
// The task's stdio is /dev/null, so it reads its own descriptor of the same pipe.
function serve_events(loop, input) {
	loop.init();
	let reader = loop.task((pipe) => {
		for (let line = input.read("line"); length(line); line = input.read("line"))
			pipe.send(line);
		return { eof: true };
	}, (msg) => {
		if (type(msg) == "string")
			serve(msg, loop);
		else
			loop.end();
	});
	let watch = null;
	watch = loop.timer(WATCH_MS, () => {
		if (reader.finished())
			loop.end();
		else
			watch.set(WATCH_MS);
	});
	loop.run();
	shutdown();
}

let loop = event_loop();
let input = loop ? fs.open("/proc/self/fd/0", "r") : null;
if (loop && !input) {
	warn(`cannot reopen stdin (${fs.error()}); serving one request at a time`);
	loop = null;
}
fs.mkdir("/tmp/uniwrt", 0o700);
fs.mkdir(RUN, 0o700);

send({
	hello: {
		proto: PROTO,
		agent: read(`${sourcepath(0, true)}/VERSION`) ?? "dev",
		release: shvars("/etc/openwrt_release").DISTRIB_RELEASE,
		caps: { events: loop != null, ops: sort(keys(OPS)), sets: sort(keys(SETS)), topics: sort(keys(TOPICS)) },
	},
});

if (loop)
	serve_events(loop, input);
else
	serve_sync();
