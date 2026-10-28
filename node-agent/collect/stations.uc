// The `stations` set: the stations of every Wi-Fi interface (nl80211 `GET_STATION`): an AP's
// clients, mesh peers and a station interface's AP, with signal, rates, counters and airtime;
// and for APs, the capabilities hostapd saw in each client's association (`get_clients`).

import * as bus from "../bus.uc";
import { phys, request, interfaces } from "../wifi.uc";
import { pick } from "../util.uc";

const STA_INFO = ["inactive_time", "connected_time", "signal", "signal_avg", "rx_bitrate", "tx_bitrate",
	"rx_bytes", "tx_bytes", "rx_bytes64", "tx_bytes64", "rx_packets", "tx_packets", "tx_retries", "tx_failed",
	"expected_throughput", "sta_flags", "plink_state", "rx_duration", "tx_duration"];
const CLIENT = ["authorized", "wmm", "ht", "vht", "he", "eht", "mfp", "rrm", "extended_capabilities"];
// `enum nl80211_iftype`
const IFTYPE_AP = 3;

function collect() {
	let stations = {}, clients = {};
	if (!length(phys()))
		return { stations: stations, clients: clients };
	for (let i in interfaces()) {
		try {
			stations[i.ifname] = map(request("NL80211_CMD_GET_STATION", "NLM_F_DUMP", { dev: i.ifname }),
				(s) => ({ mac: s.mac, info: pick(s.sta_info, STA_INFO) }));
		}
		catch (e) {
			// An interface that went down meanwhile has no stations.
			continue;
		}
		if (i.iftype != IFTYPE_AP)
			continue;
		let r = bus.call(`hostapd.${i.ifname}`, "get_clients");
		if (r.code)
			continue;
		let caps = {};
		for (let mac, client in r.ok?.clients ?? {})
			caps[mac] = pick(client, CLIENT);
		clients[i.ifname] = caps;
	}
	return { stations: stations, clients: clients };
}

export { collect };
