// The `leases` set: DHCP leases from dnsmasq's lease files (each `dnsmasq` section's `leasefile`,
// /tmp/dhcp.leases by default) and from odhcpd over ubus (`dhcp ipv6leases`, and `ipv4leases`
// where odhcpd serves DHCPv4), with the node's clock, which dnsmasq's expiry times are on.

import * as fs from "fs";
import * as uci from "uci";
import * as bus from "../bus.uc";

const DEFAULT_LEASEFILE = "/tmp/dhcp.leases";
const MAX_BYTES = 4194304;

function collect() {
	let files = [];
	uci.cursor().foreach("dhcp", "dnsmasq", (s) => {
		let file = s.leasefile ?? DEFAULT_LEASEFILE;
		if (index(files, file) < 0)
			push(files, file);
	});
	let odhcpd = {};
	for (let method in ["ipv4leases", "ipv6leases"]) {
		let r = bus.call("dhcp", method);
		odhcpd[method] = r.code ? null : r.ok;
	}
	return {
		now: clock()[0],
		dnsmasq: map(files, (file) => ({ file: file, data: fs.readfile(file, MAX_BYTES) })),
		odhcpd: odhcpd,
	};
}

export { collect };
