// Small readers shared by the agent's modules: files, sysfs attributes, shell-variable files and
// clocks.

import * as fs from "fs";

// A file's contents without surrounding whitespace, or null when it cannot be read.
function read(path) {
	let data = fs.readfile(path, 65536);
	return data == null ? null : trim(data);
}

// A JSON file, or null when it is missing or malformed.
function readjson(path) {
	let data = fs.readfile(path, 1048576);
	if (data == null)
		return null;
	try {
		return json(data);
	}
	catch (e) {
		return null;
	}
}

// `KEY='value'` lines, as in /etc/openwrt_release, as an object.
function shvars(path) {
	let vars = {};
	for (let line in split(fs.readfile(path, 65536) ?? "", "\n")) {
		let m = match(line, /^([A-Za-z_][A-Za-z0-9_]*)=(.*)$/);
		if (!m)
			continue;
		let value = trim(m[2]);
		let quote = substr(value, 0, 1);
		if ((quote == "'" || quote == '"') && length(value) >= 2 && substr(value, length(value) - 1) == quote)
			value = substr(value, 1, length(value) - 2);
		vars[m[1]] = value;
	}
	return vars;
}

// The wall clock in Unix milliseconds.
function now_ms() {
	let t = clock();
	return t[0] * 1000 + int(t[1] / 1000000);
}

// A monotonic clock in milliseconds, for measuring durations.
function mono_ms() {
	let t = clock(true);
	return t[0] * 1000 + int(t[1] / 1000000);
}

export { read, readjson, shvars, now_ms, mono_ms };
