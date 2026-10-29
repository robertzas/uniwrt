// The agent protocol, mirrored from crates/node/src/proto.rs and proto/ops.rs: one JSON object
// per line in each direction. Keep both sides in step; `PROTO` bumps on breaking changes.

import * as fs from "fs";
import { now_ms } from "./util.uc";

const PROTO = 1;

// Error codes of failed requests (`proto::code`).
const BAD_REQUEST = "bad_request";
const NOT_FOUND = "not_found";
const TIMEOUT = "timeout";
const TOO_LARGE = "too_large";
const DENIED = "denied";
const UNSUPPORTED = "unsupported";
const FAILED = "failed";

// Writes one message as one line and flushes it, so the controller sees it at once.
function send(message) {
	fs.stdout.write(sprintf("%J\n", message));
	fs.stdout.flush();
}

function answer(id, result) {
	return { id: id, ok: result };
}

function failure(id, code, msg) {
	return { id: id, err: { code: code, msg: msg } };
}

// Pushes an event, stamped with the node's clock.
function event(topic, data) {
	send({ ev: topic, ts: now_ms(), data: data });
}

export {
	PROTO, BAD_REQUEST, NOT_FOUND, TIMEOUT, TOO_LARGE, DENIED, UNSUPPORTED, FAILED,
	send, answer, failure, event
};
