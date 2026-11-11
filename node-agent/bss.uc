// The `bss` op (features/wifi.md, WLANs; D-083): turns a phy's BSSes off and on again without
// touching UCI, through hostapd's own `config_set`. hostapd takes the phy's config as netifd wrote
// it (/var/run/hostapd-<phy>[.<radio>].conf), less the BSSes named off, and adds and removes BSSes
// to match: the others keep running, the first BSS included (the next one takes its place), and
// a config without BSSes removes hostapd's interface on the phy (a mesh point there stays). Named
// none, it takes netifd's config whole, which changes nothing when all are on. A `wifi reload`
// or an apply hands hostapd netifd's config again, so the BSSes come back on.

import * as fs from "fs";
import * as bus from "./bus.uc";
import { BAD_REQUEST, NOT_FOUND, FAILED } from "./proto.uc";

const RUN = "/var/run";
// The filtered configs: they hold the keys, as netifd's do, so only root reads them.
const DIR = "/tmp/uniwrt/hostapd";

// netifd's hostapd configs of `phy`, one per radio of a multi-radio phy: [{ name, radio }].
function configs(phy) {
	let found = [];
	for (let name in fs.lsdir(RUN) ?? []) {
		let m = match(name, /^hostapd-([^.]+)(\.([0-9]+))?\.conf$/);
		if (m && m[1] == phy)
			push(found, { name: name, radio: m[3] != null ? int(m[3]) : null });
	}
	return sort(found, (a, b) => (a.radio ?? -1) - (b.radio ?? -1));
}

// `text` without the BSSes in `off`: the first BSS is `interface=`, the others `bss=`; when the
// first goes, the next becomes it. Answers the text and the BSSes kept and dropped.
function without(text, off) {
	let out = [], kept = [], dropped = [];
	let skip = false, promote = false;
	for (let line in split(text, "\n")) {
		let first = index(line, "interface=") == 0, next = index(line, "bss=") == 0;
		if (first || next) {
			let name = substr(line, first ? 10 : 4);
			skip = name in off;
			push(skip ? dropped : kept, name);
			if (skip) {
				if (first)
					promote = true;
				continue;
			}
			if (promote) {
				line = `interface=${name}`;
				promote = false;
			}
		}
		if (!skip)
			push(out, line);
	}
	return { text: join("\n", out), kept: kept, dropped: dropped };
}

function bss(args, done) {
	if (type(args.phy) != "string" || !match(args.phy, /^[A-Za-z0-9_-]+$/))
		return done(null, BAD_REQUEST, "phy must be a phy's name");
	let off = args.off ?? [];
	if (type(off) != "array" || length(filter(off, (i) => type(i) != "string")))
		return done(null, BAD_REQUEST, "off must be an array of interface names");
	let found = configs(args.phy);
	if (!length(found))
		return done(null, NOT_FOUND, `netifd has no hostapd config of ${args.phy}`);
	let radios = [];
	for (let c in found) {
		let path = `${RUN}/${c.name}`;
		let text = fs.readfile(path);
		if (text == null)
			return done(null, FAILED, `${path}: ${fs.error()}`);
		let r = without(text, off);
		if (length(r.dropped)) {
			fs.mkdir("/tmp/uniwrt", 448);
			fs.mkdir(DIR, 448);
			path = `${DIR}/${c.name}`;
			if (!fs.writefile(path, r.text))
				return done(null, FAILED, `${path}: ${fs.error()}`);
		}
		let msg = { phy: args.phy, config: path };
		if (c.radio != null)
			msg.radio = c.radio;
		let set = bus.call("hostapd", "config_set", msg);
		if (set.code)
			return done(null, set.code, set.msg);
		push(radios, { radio: c.radio, on: r.kept, off: r.dropped });
	}
	done({ radios: radios });
}

export { bss, without };
