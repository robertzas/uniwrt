// The collect sets: each gathers the raw sources of one part of the node's facts, which
// crates/node/src/facts parses. A new set is a file in collect/, a line here and a variant of
// `proto::ops::Set`.

import * as board from "./collect/board.uc";
import * as capabilities from "./collect/capabilities.uc";
import * as devices from "./collect/devices.uc";
import * as interfaces from "./collect/interfaces.uc";
import * as system from "./collect/system.uc";

const SETS = {
	board: board.collect,
	capabilities: capabilities.collect,
	devices: devices.collect,
	interfaces: interfaces.collect,
	system: system.collect,
};

export { SETS };
