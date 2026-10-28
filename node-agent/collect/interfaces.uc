// The `interfaces` set: netifd's logical interfaces (`network.interface dump`).

import * as bus from "../bus.uc";

function collect() {
	return { dump: bus.get("network.interface", "dump") };
}

export { collect };
