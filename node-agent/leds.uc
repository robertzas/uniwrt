// The `leds` op (features/network.md, Other services; D-138): every LED of the node dark, and
// back as it was. `off` saves each LED's trigger, brightness and its trigger's settings (the
// writable files of its directory, and of its subdirectories such as usbport's `ports/`) to
// STATE, then sets its trigger to `none` and its brightness to 0. An LED found lit again (an
// init script or a hotplug handler set it) is saved anew, its state being the newer one. `on`
// puts back what was saved, the trigger first (which makes its settings' files), then the
// settings, and the brightness of an LED without a trigger, and forgets it. A reboot loses STATE
// with /tmp and brings the LEDs up as the board has them; nothing here touches UCI.

import * as fs from "fs";
import { read, readjson } from "./util.uc";
import { BAD_REQUEST, FAILED } from "./proto.uc";

// Swapped by the agent tests for a tree of plain files (`paths`).
let root = "/sys/class/leds";
let state_path = "/tmp/uniwrt/leds.json";

// Entries of an LED's directory that aren't its trigger's settings.
const SKIP = {
	brightness: true, max_brightness: true, trigger: true, uevent: true, power: true,
	brightness_hw_changed: true,
};

function paths(leds, state) {
	root = leds;
	state_path = state;
}

// The trigger in use: `trigger` lists them all, the one in use in brackets.
function trigger(dir) {
	let all = read(`${dir}/trigger`);
	if (all == null)
		return null;
	let m = match(all, /\[([^\]]+)\]/);
	return m ? m[1] : (index(all, " ") < 0 ? all : null);
}

function brightness(dir) {
	let b = read(`${dir}/brightness`);
	return b == null ? null : int(b);
}

// The writable files of `dir` (and of its subdirectories, one level down) by relative path.
function settings(dir, prefix) {
	let out = {};
	for (let name in sort(fs.lsdir(dir) ?? [])) {
		if (!prefix && SKIP[name])
			continue;
		let path = `${dir}/${name}`;
		let st = fs.lstat(path);
		if (st?.type == "directory" && !prefix) {
			let sub = settings(path, `${name}/`);
			for (let k in sub)
				out[k] = sub[k];
		}
		else if (st?.type == "file" && st.perm?.user_write) {
			let value = read(path);
			if (value != null)
				out[`${prefix ?? ""}${name}`] = value;
		}
	}
	return out;
}

function names() {
	return sort(fs.lsdir(root) ?? []);
}

function dark(dir) {
	return trigger(dir) == "none" && brightness(dir) === 0;
}

// What the `system` set reports: how many LEDs, how many lit, and whether a state is saved.
function state() {
	let all = names();
	let saved = readjson(state_path);
	return {
		count: length(all),
		lit: length(filter(all, (name) => !dark(`${root}/${name}`))),
		held: length(keys(saved?.leds ?? {})) > 0,
	};
}

function save(saved) {
	fs.mkdir(substr(state_path, 0, rindex(state_path, "/")), 448);
	return fs.writefile(state_path, sprintf("%J", saved)) != null;
}

function off(done) {
	let saved = readjson(state_path);
	if (type(saved?.leds) != "object")
		saved = { leds: {} };
	let all = names();
	for (let name in all) {
		let dir = `${root}/${name}`;
		if (!dark(dir) || !(name in saved.leds))
			saved.leds[name] = {
				trigger: trigger(dir),
				brightness: brightness(dir),
				settings: settings(dir),
			};
	}
	// Saved before anything goes dark, so nothing is lost if this stops halfway.
	if (length(all) && !save(saved))
		return done(null, FAILED, `${state_path}: ${fs.error()}`);
	let failed = [];
	for (let name in all) {
		let dir = `${root}/${name}`;
		if (fs.writefile(`${dir}/trigger`, "none") == null ||
		    fs.writefile(`${dir}/brightness`, "0") == null)
			push(failed, `${name}: ${fs.error()}`);
	}
	if (length(failed))
		return done(null, FAILED, `cannot turn off ${join(", ", failed)}`);
	done(state());
}

// Puts one LED back; answers why it couldn't, or null.
function restore(name, led) {
	let dir = `${root}/${name}`;
	if (!fs.lstat(dir))
		return null;
	if (led.trigger != null && fs.writefile(`${dir}/trigger`, led.trigger) == null)
		return `${name}: ${fs.error()}`;
	// The trigger's own files first, then those of its subdirectories.
	let depth = (file) => index(file, "/") >= 0 ? 1 : 0;
	let order = sort(keys(led.settings ?? {}), (a, b) =>
		depth(a) != depth(b) ? depth(a) - depth(b) : (a < b ? -1 : (a > b ? 1 : 0)));
	for (let file in order)
		fs.writefile(`${dir}/${file}`, led.settings[file]);
	if ((led.trigger == null || led.trigger == "none") && led.brightness != null &&
	    fs.writefile(`${dir}/brightness`, `${led.brightness}`) == null)
		return `${name}: ${fs.error()}`;
	return null;
}

function on(done) {
	let saved = readjson(state_path);
	let kept = {}, failed = [];
	for (let name, led in saved?.leds ?? {}) {
		let why = restore(name, led);
		if (why) {
			kept[name] = led;
			push(failed, why);
		}
	}
	// Those that failed stay saved, so the controller tries again.
	if (length(failed)) {
		save({ leds: kept });
		return done(null, FAILED, `cannot turn on ${join(", ", failed)}`);
	}
	fs.unlink(state_path);
	done(state());
}

function leds(args, done) {
	if (type(args.off) != "bool")
		return done(null, BAD_REQUEST, "off must be true or false");
	args.off ? off(done) : on(done);
}

export { leds, state, paths };
