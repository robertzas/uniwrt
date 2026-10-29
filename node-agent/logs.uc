// Following the node's log (`log_follow`; event loop only), pushed as `{"ev":"log",…}` events
// in batches. Two modes (D-036):
// - poll (the default): logd is read over ubus every interval_ms, and entries newer than the
//   last one sent go out as logd keeps them: `{"entries":[{id,priority,source,time,msg}…],
//   "dropped":N}`, where N counts the entries lost to a gap in logd's ids.
// - stream: a task runs `logread -f -t` and its lines go out as they come, as text:
//   `{"lines":["<ctime> [<s>.<ms>] <facility>.<level> <msg>"…]}`.
// Backfill is the last N entries, in the mode's shape, before any others. The answer comes once
// the agent follows, so whatever is logged after it is pushed. When a stream's logread ends,
// `{"ev":"log.end","data":{"reason":…}}` says so and following stops.

import * as fs from "fs";
import * as bus from "./bus.uc";
import { BAD_REQUEST, UNSUPPORTED, FAILED, event } from "./proto.uc";

const LOGREAD = "/sbin/logread";
// The shell prints its pid, which logread keeps, so stopping can kill it. The only value in the
// command is the line count, an integer formatted with %d.
const STREAM = 'echo "$$" && exec /sbin/logread -f -t -l %d 2>/dev/null';
// How long logread may take to show that it follows before the answer goes out anyway.
const READY_MS = 3000;
const MAX_BACKFILL = 1000;
// The most entries one poll sends; more since the last poll count as dropped.
const MAX_BATCH = 500;
// Entries logged between a poll's two reads.
const SLACK = 32;
const DEFAULT_INTERVAL_MS = 1000;
const MIN_INTERVAL_MS = 200;
const MAX_INTERVAL_MS = 60000;
// How long stream lines gather before they go out together.
const FLUSH_MS = 100;

// { mode, timer, seen } while polling, where seen is the id of the newest entry sent (null
// until logd answers); { mode, task, pid, lines, flush, ready } while streaming, where ready
// answers the request until logread follows.
let follower = null;

// The newest `count` entries, oldest first, or null when logd does not answer.
function entries(count) {
	let r = bus.call("log", "read", { lines: count, stream: false });
	return r.code ? null : (r.ok?.log ?? []);
}

// Sends the last `count` entries; returns the newest id logd has, -1 when it has none, or null
// when it does not answer.
function backfill(count) {
	let log = entries(max(count, 1));
	if (log == null)
		return null;
	if (count && length(log))
		event("log", { entries: log, dropped: 0 });
	return length(log) ? log[-1].id : -1;
}

// One poll: the newest id first, then the entries after the last one sent. logd's ids are a
// sequence that starts again at 0 when it restarts.
function poll(f) {
	let newest = entries(1)?.[0]?.id;
	if (newest == null || newest == f.seen)
		return;
	if (f.seen == null) {
		f.seen = newest;
		return;
	}
	if (newest < f.seen)
		f.seen = -1;
	let batch = filter(entries(min(newest - f.seen + SLACK, MAX_BATCH)) ?? [], (e) => e.id > f.seen);
	if (!length(batch))
		return;
	let dropped = batch[0].id - f.seen - 1;
	f.seen = batch[-1].id;
	event("log", { entries: batch, dropped: dropped > 0 ? dropped : 0 });
}

function flush(f) {
	f.flush?.cancel();
	f.flush = null;
	if (length(f.lines))
		event("log", { lines: f.lines });
	f.lines = [];
}

// logread sends the last count + 1 entries and then follows, in one request to logd, so nothing
// falls between the backfill and the stream. Its first line shows that it follows and is
// dropped: it is the entry before the backfill (or, when logd holds fewer entries, the oldest).
function stream(loop, count, ready) {
	let f = { mode: "stream", pid: null, lines: [], flush: null, ready: ready };
	let cmd = sprintf(STREAM, count + 1);
	f.task = loop.task((pipe) => {
		let p = fs.popen(cmd, "r");
		if (!p)
			return { end: `cannot run logread: ${fs.error()}` };
		for (let line = p.read("line"); length(line); line = p.read("line"))
			pipe.send(line);
		p.close();
		return { end: "logread exited" };
	}, (msg) => {
		if (follower != f)
			return;
		if (type(msg) != "string") {
			flush(f);
			follower = null;
			let reason = msg?.end ?? "logread exited";
			return f.ready ? f.ready(reason) : event("log.end", { reason: reason });
		}
		let line = rtrim(msg, "\n");
		if (f.pid == null) {
			f.pid = match(line, /^[0-9]+$/) ? line : "";
			return;
		}
		if (f.ready)
			return f.ready();
		push(f.lines, line);
		if (length(f.lines) >= MAX_BATCH)
			flush(f);
		else
			f.flush ??= loop.timer(FLUSH_MS, () => flush(f));
	});
	return f;
}

// Stops following; a stream's logread is killed, so it does not outlive the agent.
function stop() {
	let f = follower;
	follower = null;
	if (!f)
		return;
	f.timer?.cancel();
	// A request still waiting for its stream is answered as replaced.
	f.ready?.();
	if (f.mode == "stream") {
		flush(f);
		if (f.pid)
			system(["kill", f.pid], 1000);
		f.task.kill();
	}
}

function current() {
	return { mode: follower?.mode ?? "off" };
}

// `{"mode":"poll","backfill":50,"interval_ms":1000}`; mode "off" stops. A call replaces the
// current follower.
function log_follow(args, done, loop) {
	let mode = args.mode ?? "poll";
	if (index(["poll", "stream", "off"], mode) < 0)
		return done(null, BAD_REQUEST, "mode must be poll, stream or off");
	let count = args.backfill ?? 0;
	if (type(count) != "int" || count < 0 || count > MAX_BACKFILL)
		return done(null, BAD_REQUEST, `backfill must be between 0 and ${MAX_BACKFILL}`);
	let ms = args.interval_ms ?? DEFAULT_INTERVAL_MS;
	if (type(ms) != "int" || ms < MIN_INTERVAL_MS || ms > MAX_INTERVAL_MS)
		return done(null, BAD_REQUEST, `interval_ms must be between ${MIN_INTERVAL_MS} and ${MAX_INTERVAL_MS}`);
	if (mode == "stream" && !fs.access(LOGREAD, "x"))
		return done(null, UNSUPPORTED, `${LOGREAD} is missing`);
	stop();
	if (mode == "poll") {
		let f = { mode: mode, seen: backfill(count), timer: null };
		f.timer = loop.timer(ms, () => {
			f.timer.set(ms);
			poll(f);
		});
		follower = f;
	}
	else if (mode == "stream") {
		let f = null, guard = null;
		let ready = (reason) => {
			f.ready = null;
			guard?.cancel();
			if (reason)
				return done(null, FAILED, reason);
			done(current());
		};
		f = stream(loop, count, ready);
		guard = loop.timer(READY_MS, () => f.ready?.());
		follower = f;
		return;
	}
	done(current());
}

export { log_follow, stop };
