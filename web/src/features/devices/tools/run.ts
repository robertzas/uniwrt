import { createEffect, createStore } from "solid-js";
import type { DiagRun } from "../../../api/gen/DiagRun";
import type { RunState } from "../../../api/gen/RunState";
import type { ServerMessage } from "../../../api/gen/ServerMessage";
import type { Live } from "../../../lib/live";
import type { Tone } from "../../../ui/Badge";

/**
 * The most lines a run's view keeps. The controller keeps 256 KiB of a run; this bounds the DOM
 * when its lines are short (an iperf3 test of 8 streams for 60 s prints about 5,000).
 */
export const MAX_LINES = 10_000;

/** One run as the `diag:<id>` topic keeps it. */
export interface RunViewState {
  run: DiagRun | null;
  /** The snapshot (or an error) arrived. */
  ready: boolean;
  /** The controller has no such run: forgotten, or never was. */
  missing: boolean;
}

export function emptyRunView(): RunViewState {
  return { run: null, ready: false, missing: false };
}

/**
 * Applies a `diag:<id>` message to the state (a store draft or a plain object). Returns false for a
 * delta before any snapshot, so the caller asks for one.
 */
export function applyDiag(state: RunViewState, message: ServerMessage): boolean {
  switch (message.type) {
    case "diag_snapshot":
      state.run = message.run;
      state.ready = true;
      state.missing = false;
      return true;
    case "error":
      state.ready = true;
      state.missing = true;
      return true;
    case "diag_delta": {
      const run = state.run;
      if (!run) return false;
      run.lines.push(...message.lines);
      // The controller dropped lines from the start to stay within its budget: so does the view.
      const dropped = Math.max(0, message.dropped - run.dropped);
      const excess = Math.max(dropped, run.lines.length - MAX_LINES);
      if (excess > 0) run.lines.splice(0, excess);
      run.dropped = Math.max(message.dropped, run.dropped + excess);
      if (message.end) {
        run.state = message.end.state;
        run.ended_at = message.end.ended_at;
        run.exit_code = message.end.exit_code;
        run.error = message.end.error;
      }
      return true;
    }
    default:
      return true;
  }
}

/** The live view of run `id` (none while undefined), following `id` when it changes. */
export function createRunView(live: Live, id: () => number | undefined): RunViewState {
  const [view, setView] = createStore<RunViewState>(emptyRunView());
  createEffect(id, (runId) => {
    setView(() => emptyRunView());
    if (runId === undefined) return;
    const topic = `diag:${runId}`;
    const unsubscribe = live.socket.subscribe(topic, (message) => {
      let applied = true;
      setView((draft) => {
        applied = applyDiag(draft, message);
      });
      if (!applied) live.socket.resync(topic);
    });
    return unsubscribe;
  });
  return view;
}

export const RUN_TONE: Record<RunState, Tone> = {
  running: "info",
  succeeded: "ok",
  failed: "crit",
  stopped: "neutral",
};

/** How long a run ran, or has run so far at `now`, in seconds. */
export function runSeconds(run: Pick<DiagRun, "started_at" | "ended_at">, now: number): number {
  return Math.max(0, ((run.ended_at ?? now) - run.started_at) / 1000);
}
