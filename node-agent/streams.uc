// Push events from ubus (`subscribe`, `unsubscribe`; event loop only): the notifications of
// daemons' objects, forwarded as `{"ev":"<source>.<type>","ts":…,"data":{…}}` with the data as
// the daemon sent it. Objects come and go as daemons restart (`hostapd.<ifname>` with each
// `wifi reload`), so new ones are subscribed to as ubusd announces them.

import * as bus from "./bus.uc";
import { BAD_REQUEST, event } from "./proto.uc";

// Each topic's objects: the source its events are named after, by object name, or null for
// objects it does not cover.
const TOPICS = {
	// Wi-Fi clients of each BSS: auth, assoc, sta-authorized, disassoc, and radar detection.
	// Probe requests are dropped: a busy AP sends dozens a second.
	hostapd: {
		source: (path) => match(path, /^hostapd\.[^.]+$/) ? "hostapd" : null,
		drop: { probe: true },
	},
	// Interfaces going up, down or changing (interface.update carries the interface's status),
	// and devices appearing, going up or down, and their links. Only the main interface object:
	// each network.interface.<name> object repeats its notifications.
	netifd: {
		source: (path) => ({ "network.interface": "netifd", "network.device": "netifd.device" })[path],
	},
	// DHCP leases acked and released, by dnsmasq (IPv4) and odhcpd.
	dhcp: {
		source: (path) => ({ dnsmasq: "dnsmasq", dhcp: "odhcpd" })[path],
	},
};

// The subscribers of each active topic, by object name: a notification does not say which
// object sent it, so each object has a subscriber of its own. And the listener for new objects,
// while any topic is active.
let active = {};
let added = null;

function watch(conn, loop, name, path) {
	let topic = TOPICS[name];
	let source = topic.source(path);
	if (!source)
		return;
	let subs = active[name];
	subs[path]?.remove();
	let sub = null;
	sub = conn.subscriber((req) => {
		if (topic.drop?.[req.type])
			return;
		let data = req.data ?? {};
		// Every hostapd notification is about one BSS; older builds may not name it.
		if (source == "hostapd")
			data.ifname ??= substr(path, length("hostapd."));
		event(`${source}.${req.type}`, data);
	}, () => {
		// The object is gone: free its subscriber once this callback has returned.
		loop.timer(0, () => {
			if (subs[path] == sub) {
				sub.remove();
				delete subs[path];
			}
		});
	});
	if (sub.subscribe(path))
		subs[path] = sub;
	else
		sub.remove();
}

function topics_of(args) {
	let names = args.topics;
	if (type(names) != "array")
		return { err: "topics must be an array" };
	for (let name in names)
		if (type(name) != "string" || !TOPICS[name])
			return { err: `unknown topic ${name}` };
	return { ok: names };
}

function current() {
	return { topics: sort(keys(active)) };
}

function subscribe(args, done, loop) {
	let topics = topics_of(args);
	if (topics.err)
		return done(null, BAD_REQUEST, topics.err);
	let names = topics.ok;
	let c = bus.connection();
	if (c.code)
		return done(null, c.code, c.msg);
	let conn = c.ok;
	// The listener comes first, so an object that appears while the others are listed is not
	// missed; one both listed and announced gets a single subscriber, the later one.
	added ??= conn.listener("ubus.object.add", (ev, data) => {
		for (let name in active)
			watch(conn, loop, name, data?.path ?? "");
	});
	let fresh = filter(names, (name) => !active[name]);
	for (let name in fresh)
		active[name] = {};
	if (length(fresh))
		for (let path in conn.list() ?? [])
			for (let name in fresh)
				watch(conn, loop, name, path);
	done(current());
}

function unsubscribe(args, done) {
	let topics = topics_of(args);
	if (topics.err)
		return done(null, BAD_REQUEST, topics.err);
	let names = topics.ok;
	for (let name in names) {
		for (let path, sub in active[name] ?? {})
			sub.remove();
		delete active[name];
	}
	if (added && !length(active)) {
		added.remove();
		added = null;
	}
	done(current());
}

export { TOPICS, subscribe, unsubscribe };
