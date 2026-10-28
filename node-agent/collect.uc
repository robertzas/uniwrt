// The collect sets: each gathers the raw sources of one part of the node's facts, which
// crates/node/src/facts parses. A new set is a file in collect/, a line here and a variant of
// `proto::ops::Set`.

import * as batman from "./collect/batman.uc";
import * as board from "./collect/board.uc";
import * as capabilities from "./collect/capabilities.uc";
import * as devices from "./collect/devices.uc";
import * as interfaces from "./collect/interfaces.uc";
import * as leases from "./collect/leases.uc";
import * as neighbors from "./collect/neighbors.uc";
import * as phys from "./collect/phys.uc";
import * as radios from "./collect/radios.uc";
import * as stations from "./collect/stations.uc";
import * as system from "./collect/system.uc";

const SETS = {
	batman: batman.collect,
	board: board.collect,
	capabilities: capabilities.collect,
	devices: devices.collect,
	interfaces: interfaces.collect,
	leases: leases.collect,
	neighbors: neighbors.collect,
	phys: phys.collect,
	radios: radios.collect,
	stations: stations.collect,
	system: system.collect,
};

export { SETS };
