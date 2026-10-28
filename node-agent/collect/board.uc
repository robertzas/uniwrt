// The `board` set: what the node is (`system board`, the release file and board.json).

import * as bus from "../bus.uc";
import { readjson, shvars } from "../util.uc";

function collect() {
	return {
		board: bus.get("system", "board"),
		release: shvars("/etc/openwrt_release"),
		board_json: readjson("/etc/board.json"),
	};
}

export { collect };
