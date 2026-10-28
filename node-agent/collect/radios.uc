// The `radios` set: each radio's configuration and state from netifd (`network.wireless
// status`), the phys' interfaces from nl80211 (`GET_INTERFACE`), and the survey of each phy's
// channel with the share of time it was busy since the previous collection. netifd's reply holds
// the Wi-Fi keys, so only the options that facts use are kept.

import * as bus from "../bus.uc";
import { NOT_FOUND } from "../proto.uc";
import { phys, request, interfaces } from "../wifi.uc";
import { pick } from "../util.uc";

const RADIO = ["band", "channel", "htmode", "country", "txpower", "path", "phy"];
const IFACE = ["mode", "ssid", "mesh_id", "encryption", "network", "hidden"];
const NL_IFACE = ["ifname", "wiphy", "iftype", "mac", "ssid", "wiphy_freq", "channel_width", "center_freq1",
	"center_freq2", "wiphy_tx_power_level"];
const SURVEY = ["frequency", "noise", "time", "busy", "in_use"];
// Less channel time than this since the previous sample keeps the previous share rather than
// reporting noise.
const MIN_TIME_MS = 1000;

// The previous survey sample of each phy: { frequency, time, busy, percent }.
let samples = {};

function wireless() {
	let reply = bus.call("network.wireless", "status");
	// Without wifi-scripts (25.12 images without Wi-Fi), netifd has no wireless object.
	if (reply.code == NOT_FOUND)
		return {};
	if (reply.code)
		die(`ubus network.wireless status: ${reply.msg}`);
	let radios = {};
	for (let name, radio in reply.ok ?? {})
		radios[name] = {
			up: radio.up,
			pending: radio.pending,
			disabled: radio.disabled,
			config: pick(radio.config, RADIO),
			interfaces: map(radio.interfaces ?? [], (i) => ({
				section: i.section,
				ifname: i.ifname,
				config: pick(i.config, IFACE),
			})),
		};
	return radios;
}

// The survey entry of the channel `iface` operates on, with `busy_percent`.
function survey(phy, iface) {
	let entry = null;
	for (let e in request("NL80211_CMD_GET_SURVEY", "NLM_F_DUMP", { dev: iface.ifname })) {
		let info = e.survey_info;
		if (info?.in_use || (!entry && info?.frequency == iface.wiphy_freq))
			entry = info;
	}
	if (!entry)
		return null;
	let s = pick(entry, SURVEY);
	let prev = samples[phy];
	if (!prev || prev.frequency != s.frequency || s.time < prev.time)
		samples[phy] = { frequency: s.frequency, time: s.time, busy: s.busy, percent: null };
	else if (s.time - prev.time >= MIN_TIME_MS)
		samples[phy] = {
			frequency: s.frequency,
			time: s.time,
			busy: s.busy,
			percent: 100.0 * (s.busy - prev.busy) / (s.time - prev.time),
		};
	s.busy_percent = samples[phy].percent;
	return s;
}

function collect() {
	let found = phys(), ifaces = [], surveys = {};
	if (length(found)) {
		let names = {};
		for (let p in found)
			names[p.index] = p.name;
		for (let i in interfaces()) {
			push(ifaces, pick(i, NL_IFACE));
			let phy = names[i.wiphy];
			if (phy && surveys[phy] == null && i.wiphy_freq) {
				try {
					surveys[phy] = survey(phy, i);
				}
				catch (e) {
					// A phy whose interface went down meanwhile has no survey this time.
				}
			}
		}
	}
	return { wireless: wireless(), phys: found, interfaces: ifaces, surveys: surveys };
}

export { collect };
