// The `phys` set: the radio hardware (nl80211 `GET_WIPHY`) per band: its channels with their
// regulatory flags and power limits, and its HT, VHT, HE and EHT capabilities. It changes rarely
// and its dump is large (command lists, vendor data), so it is collected apart from the radios'
// state, keeping only what crates/node/src/facts/phys.rs reads.

import { phys, request } from "../wifi.uc";
import { pick } from "../util.uc";

const FREQ = ["freq", "max_tx_power", "disabled", "no_ir", "radar", "dfs_state"];
const IFTYPE_DATA = ["iftypes", "he_cap_phy", "he_cap_mcs_set", "eht_cap_phy"];

// One entry of `wiphy_bands`, which is indexed by `enum nl80211_band`; absent bands are null.
function band(raw) {
	if (raw == null)
		return null;
	return {
		freqs: map(raw.freqs ?? [], (f) => pick(f, FREQ)),
		ht_capa: raw.ht_capa,
		ht_mcs_set: pick(raw.ht_mcs_set, ["rx_mcs_indexes"]),
		vht_capa: raw.vht_capa,
		vht_mcs_set: pick(raw.vht_mcs_set, ["rx_mcs_set"]),
		iftype_data: map(raw.iftype_data ?? [], (d) => pick(d, IFTYPE_DATA)),
	};
}

function collect() {
	let wiphys = [];
	if (!length(phys()))
		return { wiphys: wiphys };
	for (let w in request("NL80211_CMD_GET_WIPHY", "NLM_F_DUMP", { split_wiphy_dump: true }))
		if (w?.wiphy_name)
			push(wiphys, { wiphy: w.wiphy, name: w.wiphy_name, bands: map(w.wiphy_bands ?? [], band) });
	return { wiphys: wiphys };
}

export { collect };
