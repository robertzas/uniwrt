import { createStore } from "solid-js";
import type { AppliedRevision } from "../api/gen/AppliedRevision";
import type { Drift } from "../api/gen/Drift";
import type { DriftMute } from "../api/gen/DriftMute";
import type { DriftState } from "../api/gen/DriftState";
import type { NodeId } from "../api/gen/NodeId";
import type { NodeMode } from "../api/gen/NodeMode";
import type { RevisionMeta } from "../api/gen/RevisionMeta";
import type { ServerMessage } from "../api/gen/ServerMessage";
import type { LiveSocket } from "./ws";

/**
 * The `config` topic (D-071, D-074): the site's latest revision, the revision each node holds,
 * and every drift and mute (empty where the server watches none).
 */
export interface ConfigState {
  head: RevisionMeta | null;
  applied: Record<NodeId, AppliedRevision>;
  drift: DriftState;
  /** The first message arrived. */
  ready: boolean;
}

/** Applies a `config` message, which is whole, to the state (a store draft or a plain object). */
export function applyConfig(state: ConfigState, message: ServerMessage) {
  if (message.type !== "config") return;
  state.head = message.head;
  state.applied = Object.fromEntries(message.applied.map((held) => [held.node, held]));
  state.drift = message.drift ?? { drifts: [], muted: [] };
  state.ready = true;
}

/** The `config` topic as a store, subscribed for as long as `socket` is open. */
export function createConfig(socket: LiveSocket): ConfigState {
  const [config, setConfig] = createStore<ConfigState>({
    head: null,
    applied: {},
    drift: { drifts: [], muted: [] },
    ready: false,
  });
  socket.subscribe("config", (message) => {
    setConfig((draft) => applyConfig(draft, message));
  });
  return config;
}

/** Where a node stands against the latest revision. */
export type Holding =
  /** Observe mode: applies leave it alone. */
  | { kind: "observed" }
  /** Managed, and no apply has reached it yet. */
  | { kind: "never" }
  | { kind: "latest"; revision: number }
  /** It holds the revision the latest one restores, so the same document (D-071). */
  | { kind: "copy"; revision: number; head: number }
  | { kind: "behind"; revision: number; head: number };

export function holding(state: ConfigState, node: NodeId, mode: NodeMode): Holding {
  if (mode === "observe") return { kind: "observed" };
  const held = state.applied[node];
  if (!held) return { kind: "never" };
  const head = state.head;
  if (!head || held.revision >= head.id) return { kind: "latest", revision: held.revision };
  if (head.kind === "restore" && head.restored_from === held.revision) {
    return { kind: "copy", revision: held.revision, head: head.id };
  }
  return { kind: "behind", revision: held.revision, head: head.id };
}

/** A node the latest revision has yet to reach: managed, and behind or never applied. */
export function isPending(where: Holding): boolean {
  return where.kind === "never" || where.kind === "behind";
}

interface FleetNode {
  id: NodeId;
  mode: NodeMode;
}

/** The managed nodes the latest revision has yet to reach (the top bar's pending changes). */
export function pendingNodes<T extends FleetNode>(state: ConfigState, nodes: readonly T[]): T[] {
  if (!state.ready || !state.head) return [];
  return nodes.filter((node) => isPending(holding(state, node.id, node.mode)));
}

/**
 * The revision "discard" goes back to: the newest one a managed node holds, while the latest is
 * newer and not already its copy; undefined when there is nothing to discard.
 */
export function lastApplied(state: ConfigState, nodes: readonly FleetNode[]): number | undefined {
  const head = state.head;
  if (!head) return undefined;
  const held = nodes
    .filter((node) => node.mode === "managed")
    .map((node) => state.applied[node.id]?.revision)
    .filter((revision): revision is number => revision !== undefined);
  if (held.length === 0) return undefined;
  const newest = Math.max(...held);
  if (newest >= head.id) return undefined;
  if (head.kind === "restore" && head.restored_from === newest) return undefined;
  return newest;
}

/** The node's drifts, in the server's order (by package). */
export function driftsOf(state: ConfigState, node: NodeId): Drift[] {
  return state.drift.drifts.filter((drift) => drift.node === node);
}

/** The node's packages whose drift isn't watched. */
export function mutesOf(state: ConfigState, node: NodeId): DriftMute[] {
  return state.drift.muted.filter((mute) => mute.node === node);
}

/** The drifts that want attention: not acknowledged (the top bar counts them). */
export function unacknowledged(state: ConfigState): Drift[] {
  return state.drift.drifts.filter((drift) => !drift.acknowledged);
}
