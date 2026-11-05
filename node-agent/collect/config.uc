// The `config` set: the SHA-256 of each UCI package in /etc/config, for drift detection
// (features/lifecycle.md, Drift). A file is hashed again only once its inode, size, mtime or
// ctime moved, so a tick costs one stat per file and `sha256sum` runs only after a change. A
// file changed in the last few seconds isn't cached: a second write within the same second
// (same size, same inode) would leave the stamp as it was.

import * as fs from "fs";
import * as exec from "../exec.uc";

const DIR = "/etc/config";
// How old a change must be for its hash to be cached, in seconds.
const SETTLED_S = 2;

// package → { stamp, hash }
let cache = {};

function stamp(st) {
	return `${st.inode}:${st.size}:${st.mtime}:${st.ctime}`;
}

// `<hex>  <path>` lines, by package.
function hashes(names) {
	let paths = map(names, (name) => `${DIR}/${name}`);
	let r = exec.run({ argv: ["sha256sum", ...paths], timeout_ms: 10000, max_output: 1048576 });
	if (r.err)
		die(r.err[1]);
	if (r.ok.code != 0)
		die(`sha256sum failed: ${trim(r.ok.stderr)}`);
	let out = {};
	for (let line in split(r.ok.stdout, "\n")) {
		let m = match(line, /^([0-9a-f]{64})  \/etc\/config\/(.+)$/);
		if (m)
			out[m[2]] = m[1];
	}
	return out;
}

function collect() {
	let now = time();
	let files = {}, stale = [], stamps = {}, seen = {};
	for (let name in fs.lsdir(DIR) ?? []) {
		// The names libuci accepts as packages.
		if (!match(name, /^[A-Za-z0-9_-]+$/))
			continue;
		let st = fs.stat(`${DIR}/${name}`);
		if (!st || st.type != "file")
			continue;
		seen[name] = true;
		let key = stamp(st);
		if (cache[name]?.stamp == key) {
			files[name] = cache[name].hash;
			continue;
		}
		delete cache[name];
		push(stale, name);
		stamps[name] = { key: key, settled: now - max(st.mtime, st.ctime) >= SETTLED_S };
	}
	for (let name in keys(cache))
		if (!seen[name])
			delete cache[name];
	if (length(stale)) {
		let found = hashes(stale);
		for (let name in stale) {
			let hash = found[name];
			if (hash == null)
				continue;
			files[name] = hash;
			if (stamps[name].settled)
				cache[name] = { stamp: stamps[name].key, hash: hash };
		}
	}
	return { files: files };
}

export { collect };
