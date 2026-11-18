import { type Accessor, createEffect } from "solid-js";
import type { ControllerUpdate } from "../api/gen/ControllerUpdate";
import type { RestoreState } from "../api/gen/RestoreState";

/** How often an install is followed. */
export const INSTALL_POLL_MS = 2000;
/**
 * How long a restart may keep the controller away: the launcher's trial waits up to three
 * minutes for the new release to answer, then soaks it, or rolls back and starts the old one.
 */
export const RESTART_LIMIT_MS = 6 * 60_000;

/** How an install the page started ended. */
export type InstallOutcome =
  | { kind: "running"; version: string }
  | { kind: "rolled_back"; version: string; reason: string }
  | { kind: "failed"; version: string; reason: string }
  | { kind: "lost"; version: string };

/**
 * What a read of the controller tells of the install of `version`: running it, back on the old
 * release after a failed trial, refused before the restart, or still under way (`undefined`).
 */
export function installOutcome(
  version: string,
  update: ControllerUpdate,
): InstallOutcome | undefined {
  if (update.current === version) return { kind: "running", version };
  if (update.failure?.version === version) {
    return { kind: "rolled_back", version, reason: update.failure.reason };
  }
  const progress = update.progress;
  if (progress.state === "failed" && progress.version === version) {
    return { kind: "failed", version, reason: progress.reason };
  }
  return undefined;
}

/**
 * Reads the controller again every few seconds while `version` is being installed, through
 * `fetch`, until [`installOutcome`] tells how it ended; reads that fail (the controller is
 * restarting) are expected, until [`RESTART_LIMIT_MS`] has passed without an outcome.
 */
export function followInstall(options: {
  version: Accessor<string | undefined>;
  fetch: (signal: AbortSignal) => Promise<ControllerUpdate>;
  onUpdate: (update: ControllerUpdate) => void;
  onOutcome: (outcome: InstallOutcome) => void;
  now?: () => number;
}) {
  const now = options.now ?? Date.now;
  createEffect(options.version, (version) => {
    if (!version) return;
    const controller = new AbortController();
    const started = now();
    const timer = setInterval(async () => {
      try {
        const update = await options.fetch(controller.signal);
        if (controller.signal.aborted) return;
        options.onUpdate(update);
        const outcome = installOutcome(version, update);
        if (outcome) options.onOutcome(outcome);
      } catch {
        if (controller.signal.aborted) return;
        if (now() - started > RESTART_LIMIT_MS) options.onOutcome({ kind: "lost", version });
      }
    }, INSTALL_POLL_MS);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  });
}

/**
 * Follows the restart that puts a staged controller backup in place (D-129): reads `/health`
 * every few seconds and calls `onBack` once the controller answers after being away, or answers
 * with the restore no longer staged (a restart quicker than a read, or the restored controller
 * refusing a state read it no longer allows); `onLost` after [`RESTART_LIMIT_MS`] without either.
 * Returns a function that stops following.
 */
export function followRestore(options: {
  health: (signal: AbortSignal) => Promise<unknown>;
  state: (signal: AbortSignal) => Promise<RestoreState>;
  onBack: () => void;
  onLost: () => void;
  now?: () => number;
}): () => void {
  const now = options.now ?? Date.now;
  const controller = new AbortController();
  const started = now();
  let away = false;
  let busy = false;
  const stop = () => {
    controller.abort();
    clearInterval(timer);
  };
  const end = (then: () => void) => {
    stop();
    then();
  };
  const read = async () => {
    try {
      await options.health(controller.signal);
    } catch {
      away = true;
      return false;
    }
    if (away) return true;
    try {
      return (await options.state(controller.signal)).state !== "ready";
    } catch {
      return true;
    }
  };
  const timer = setInterval(async () => {
    if (busy) return;
    busy = true;
    const back = await read();
    busy = false;
    if (controller.signal.aborted) return;
    if (back) end(options.onBack);
    else if (now() - started > RESTART_LIMIT_MS) end(options.onLost);
  }, INSTALL_POLL_MS);
  return stop;
}
