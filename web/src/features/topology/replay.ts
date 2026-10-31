import { type Accessor, createSignal, onSettled } from "solid-js";
import type { TopologyTimeline } from "../../api/gen/TopologyTimeline";
import { api } from "../../lib/api";
import { type Graph, graphOf } from "./graph";

/** The replay of the last 24 h: the live map until the user picks a time. */
export interface Replay {
  /** How far back the history goes; refreshed every minute. */
  timeline: Accessor<TopologyTimeline | undefined>;
  /** The time shown, or `null` for live. */
  at: Accessor<number | null>;
  /** The topology at that time, once loaded (the previous one until then). */
  graph: Accessor<Graph | undefined>;
  failed: Accessor<boolean>;
  /** Shows the topology at `ms`, or live again with `null`. */
  seek(ms: number | null): void;
}

const TIMELINE_EVERY_MS = 60_000;
/** A slider sends many values while dragged: only the one it rests on is fetched. */
const SEEK_DELAY_MS = 150;

/**
 * Polls the timeline into a signal rather than a query: async data the page reads would hold
 * navigations away from it while in flight (D-048).
 */
export function createReplay(): Replay {
  const [timeline, setTimeline] = createSignal<TopologyTimeline>();
  const [at, setAt] = createSignal<number | null>(null);
  const [graph, setGraph] = createSignal<Graph>();
  const [failed, setFailed] = createSignal(false);
  let pending: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const refresh = () => {
    api.topology.timeline().then(setTimeline, () => {});
  };
  onSettled(() => {
    refresh();
    const poll = setInterval(refresh, TIMELINE_EVERY_MS);
    return () => {
      clearInterval(poll);
      clearTimeout(timer);
      pending?.abort();
    };
  });

  const load = (ms: number) => {
    pending?.abort();
    const controller = new AbortController();
    pending = controller;
    api.topology.at(ms, controller.signal).then(
      (topology) => {
        if (controller.signal.aborted) return;
        setGraph(graphOf(topology));
        setFailed(false);
      },
      () => {
        if (!controller.signal.aborted) setFailed(true);
      },
    );
  };

  return {
    timeline,
    at,
    graph,
    failed,
    seek(ms) {
      clearTimeout(timer);
      setAt(ms);
      if (ms === null) {
        pending?.abort();
        setGraph(undefined);
        setFailed(false);
        refresh();
        return;
      }
      timer = setTimeout(() => load(ms), SEEK_DELAY_MS);
    },
  };
}
