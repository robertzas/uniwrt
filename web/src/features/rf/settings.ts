import type { RfSettings } from "../../api/gen/RfSettings";
import { api } from "../../lib/api";

/**
 * Saves of the RF settings (the scan schedule, the nightly optimizer, DFS). Each control sends
 * the whole settings, and the page shows them again only once the `rf` topic pushes them: a
 * second change made meanwhile would send the settings from before the first and undo it. So
 * each change is laid over the last save's settings while that is recent, and saves go in order.
 */

/** How long a save's settings stay the base after it is answered: the topic's push comes first. */
export const SETTLE_MS = 3_000;

/** Saves `change` over the settings shown (or the last save's, while recent); resolves the stored settings. */
export type SaveRfSettings = (
  shown: RfSettings,
  change: Partial<RfSettings>,
) => Promise<RfSettings>;

/** The page's saver: one per mounted page, so a page opened later starts from what it shows. */
export function createRfSaver(): SaveRfSettings {
  /** The last save's settings, until it settles. */
  let last: RfSettings | undefined;
  let settle: ReturnType<typeof setTimeout> | undefined;
  let queue: Promise<unknown> = Promise.resolve();
  return (shown, change) => {
    const next: RfSettings = { ...(last ?? shown), ...change };
    last = next;
    clearTimeout(settle);
    const saved = queue.catch(() => undefined).then(() => api.rf.settings(next));
    queue = saved;
    const done = () => {
      if (last !== next) return;
      clearTimeout(settle);
      settle = setTimeout(() => {
        if (last === next) last = undefined;
      }, SETTLE_MS);
    };
    return saved.then(
      (stored) => {
        done();
        return stored;
      },
      (error: unknown) => {
        if (last === next) last = undefined;
        throw error;
      },
    );
  };
}
