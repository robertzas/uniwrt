// Commands as argv arrays, never shell strings, with a timeout and bounded output. A command's
// stdio never reaches the agent's stdout, which carries the protocol: its output goes to files
// under RUN, read back once it exits.

import * as fs from "fs";
import { mono_ms } from "./util.uc";
import { TIMEOUT, TOO_LARGE, FAILED } from "./proto.uc";

const RUN = "/tmp/uniwrt/run";
const DEFAULT_TIMEOUT_MS = 10000;
const MAX_TIMEOUT_MS = 600000;
const DEFAULT_MAX_OUTPUT = 1048576;
const MAX_OUTPUT = 16777216;

// $1 is the size limit of each output file in 512-byte blocks and $2 the files' path prefix; the
// command follows. A command that writes more gets SIGXFSZ.
const WRAP = 'ulimit -f "$1" && o=$2 && shift 2 && exec "$@" </dev/null >"$o.out" 2>"$o.err"';
const QUIET = 'exec "$@" </dev/null >/dev/null 2>&1';

const SIGKILL = 9;
const SIGXFSZ = 25;

let seq = 0;

// Checks and completes an `exec` request's arguments: returns them, or { err: message }.
function parse(args) {
	let argv = args.argv;
	if (type(argv) != "array" || !length(argv))
		return { err: "argv must be a non-empty array" };
	for (let arg in argv)
		if (type(arg) != "string" || index(arg, "\0") >= 0)
			return { err: "argv must hold strings without NUL bytes" };
	let timeout_ms = args.timeout_ms ?? DEFAULT_TIMEOUT_MS;
	if (type(timeout_ms) != "int" || timeout_ms < 1 || timeout_ms > MAX_TIMEOUT_MS)
		return { err: `timeout_ms must be between 1 and ${MAX_TIMEOUT_MS}` };
	let max_output = args.max_output ?? DEFAULT_MAX_OUTPUT;
	if (type(max_output) != "int" || max_output < 0 || max_output > MAX_OUTPUT)
		return { err: `max_output must be between 0 and ${MAX_OUTPUT}` };
	return { argv: argv, timeout_ms: timeout_ms, max_output: max_output };
}

// Runs a parsed request to completion, blocking: returns { ok: { code, signal, stdout,
// stderr } } or { err: [code, msg] }. Under the event loop it runs in a task of its own.
function run(req) {
	// The pid keeps names unique across agents and tasks; the sequence, within one.
	let base = sprintf("%s/%s.%d", RUN, fs.readlink("/proc/self"), ++seq);
	let blocks = int(req.max_output / 512) + 1;
	let cmd = ["/bin/sh", "-c", WRAP, "sh", sprintf("%d", blocks), base];
	for (let arg in req.argv)
		push(cmd, arg);
	let start = mono_ms();
	let rc = system(cmd, req.timeout_ms);
	let elapsed = mono_ms() - start;
	let stdout = fs.readfile(`${base}.out`, req.max_output + 1) ?? "";
	let stderr = fs.readfile(`${base}.err`, req.max_output + 1) ?? "";
	fs.unlink(`${base}.out`);
	fs.unlink(`${base}.err`);
	let name = req.argv[0];
	if (type(rc) != "int")
		return { err: [FAILED, `cannot run ${name}`] };
	if (rc == -SIGKILL && elapsed >= req.timeout_ms)
		return { err: [TIMEOUT, `${name} did not exit within ${req.timeout_ms} ms`] };
	if (rc == -SIGXFSZ || length(stdout) > req.max_output || length(stderr) > req.max_output)
		return { err: [TOO_LARGE, `${name} wrote more than ${req.max_output} bytes`] };
	return { ok: { code: rc >= 0 ? rc : null, signal: rc < 0 ? -rc : null, stdout: stdout, stderr: stderr } };
}

// Runs a command for its exit code only, discarding its output; null when it could not run.
function status(argv, timeout_ms) {
	let cmd = ["/bin/sh", "-c", QUIET, "sh"];
	for (let arg in argv)
		push(cmd, arg);
	let rc = system(cmd, timeout_ms);
	return type(rc) == "int" ? rc : null;
}

export { RUN, parse, run, status };
