// The `system` set: resource usage (`system info`), the node's clock, CPU load since the previous
// collection and the thermal sensors.

import * as fs from "fs";
import * as bus from "../bus.uc";
import { read } from "../util.uc";

// Fewer jiffies than this since the previous sample (a collection right after another) keep
// the previous percentage rather than reporting noise.
const MIN_JIFFIES = 50;

let sample = null;
let percent = null;

// The aggregate `cpu` line of /proc/stat as [busy, total] jiffies.
function cpu() {
	let line = split(fs.readfile("/proc/stat", 1024) ?? "", "\n")[0];
	let fields = split(trim(line), /\s+/);
	if (fields[0] != "cpu" || length(fields) < 5)
		return null;
	// user nice system idle iowait irq softirq steal; guest time is already in user.
	let total = 0;
	for (let i = 1; i <= 8 && i < length(fields); i++)
		total += int(fields[i]);
	let idle = int(fields[4]) + int(fields[5] ?? 0);
	return [total - idle, total];
}

function thermal() {
	let zones = [];
	for (let zone in fs.glob("/sys/class/thermal/thermal_zone*") ?? []) {
		let temp = read(`${zone}/temp`);
		if (temp != null)
			push(zones, { type: read(`${zone}/type`), temp: int(temp) });
	}
	return zones;
}

function collect() {
	let now = cpu();
	if (now && sample && now[1] - sample[1] >= MIN_JIFFIES) {
		percent = 100.0 * (now[0] - sample[0]) / (now[1] - sample[1]);
		sample = now;
	}
	else if (now && !sample)
		sample = now;
	return {
		info: bus.get("system", "info"),
		clock: clock(),
		cpu_percent: percent,
		thermal: thermal(),
	};
}

export { collect };
