// ubus calls on one shared connection, with errors mapped to the protocol's codes. Subscriptions
// (streams.uc) use the same connection, which the event loop then serves.

import * as ubus from "ubus";
import { BAD_REQUEST, NOT_FOUND, TIMEOUT, DENIED, UNSUPPORTED, FAILED } from "./proto.uc";

// How long a call may take; ubus applies it per connection.
const TIMEOUT_S = 10;

// The texts of ubus_strerror(), which prefix the module's error messages.
const CODES = {
	"Not found": NOT_FOUND,
	"Method not found": NOT_FOUND,
	"Request timed out": TIMEOUT,
	"No response": TIMEOUT,
	"Permission denied": DENIED,
	"Invalid argument": BAD_REQUEST,
	"Invalid command": BAD_REQUEST,
	"Operation not supported": UNSUPPORTED,
};

let conn = null;

// The shared connection, made on first use: returns { ok: connection } or { code, msg }.
function connection() {
	if (!conn) {
		conn = ubus.connect(null, TIMEOUT_S);
		if (!conn)
			return { code: FAILED, msg: `cannot connect to ubus: ${ubus.error()}` };
	}
	return { ok: conn };
}

// Calls `method` on the object `path`: returns { ok: result } or { code, msg }.
function call(path, method, data) {
	let c = connection();
	if (c.code)
		return c;
	// Reading the last error clears it, so a call that returns nothing is told from a failure.
	conn.error();
	let result = conn.call(path, method, data ?? {});
	if (result != null)
		return { ok: result };
	let err = conn.error();
	if (err == null)
		return { ok: null };
	let prefix = split(err, ":")[0];
	if (prefix == "Connection failed")
		conn = null;
	return { code: CODES[prefix] ?? FAILED, msg: err };
}

// Like `call`, but a failure raises, failing the collect set that made the call.
function get(path, method, data) {
	let r = call(path, method, data);
	if (r.code)
		die(`ubus ${path} ${method}: ${r.msg}`);
	return r.ok;
}

export { connection, call, get };
