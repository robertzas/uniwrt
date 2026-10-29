// Periodic collection (`set_interval`; event loop only): sets collected on a schedule and pushed
// as `{"tick":{"sets":{…},"errors":{…}}}`, the shape of a `collect` answer. Each interval is one
// timer whose tick carries all of its sets, and a set has at most one interval. Sets are
// collected in the loop, as `collect` is: they take milliseconds, and their commands are bounded
// by timeouts.

import * as collect from "./collect.uc";
import { BAD_REQUEST, send } from "./proto.uc";

const MIN_MS = 1000;
const MAX_MS = 86400000;

// The interval of each scheduled set, and the timer of each interval in use.
let intervals = {};
let timers = {};

function due(ms) {
	return sort(filter(keys(intervals), (set) => intervals[set] == ms));
}

function start(loop, ms) {
	let timer = null;
	timer = loop.timer(ms, () => {
		timer.set(ms);
		send({ tick: collect.run(due(ms)) });
	});
	timers[`${ms}`] = timer;
}

// `{"interval_ms":2000,"sets":["radios","stations"]}` moves those sets to that interval, keeping
// the phase of a timer already running at it; 0 stops collecting them. Answers every set's
// interval.
function set_interval(args, done, loop) {
	let ms = args.interval_ms;
	if (type(ms) != "int" || (ms != 0 && (ms < MIN_MS || ms > MAX_MS)))
		return done(null, BAD_REQUEST, `interval_ms must be 0 or between ${MIN_MS} and ${MAX_MS}`);
	let sets = args.sets;
	if (type(sets) != "array")
		return done(null, BAD_REQUEST, "sets must be an array");
	for (let set in sets)
		if (type(set) != "string" || type(collect.SETS[set]) != "function")
			return done(null, BAD_REQUEST, `unknown set ${set}`);
	for (let set in sets) {
		if (ms)
			intervals[set] = ms;
		else
			delete intervals[set];
	}
	for (let key in keys(timers)) {
		if (!length(due(int(key)))) {
			timers[key].cancel();
			delete timers[key];
		}
	}
	if (ms && length(sets) && !timers[`${ms}`])
		start(loop, ms);
	done({ intervals: intervals });
}

export { set_interval };
