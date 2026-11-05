import { createStore } from "solid-js";
import type { AppliedRevision } from "../api/gen/AppliedRevision";
import type { NodeId } from "../api/gen/NodeId";
import type { NodeMode } from "../api/gen/NodeMode";
import type { RevisionMeta } from "../api/gen/RevisionMeta";
import type { ServerMessage } from "../api/gen/ServerMessage";
import type { LiveSocket } from "./ws";

/** The `config` topic (D-071): the site's latest revision and the revision each node holds. */
export interface ConfigState {
  head: RevisionMeta | null;
  applied: Record<NodeId, AppliedRevision>;
  /** The first message arrived. */
  ready: boolean;
}

/** Applies a `config` message, which is whole, to the state (a store draft or a plain object). */
export function applyConfig(state: ConfigState, message: ServerMessage) {
  if (message.type !== "config") return;
  state.head = message.head;
  state.applied = Object.fromEntries(message.applied.map((held) => [held.node, held]));
  state.ready = true;
}

/** The `config` topic as a store, subscribed for as long as `socket` is open. */
export function createConfig(socket: LiveSocket): ConfigState {
  const [config, setConfig] = createStore<ConfigState>({ head: null, applied: {}, ready: false });
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
  | { kind: "behind"; revision: number; head: number };

export function holding(state: ConfigState, node: NodeId, mode: NodeMode): Holding {
  if (mode === "observe") return { kind: "observed" };
  const held = state.applied[node];
  if (!held) return { kind: "never" };
  const head = state.head?.id;
  return head === undefined || held.revision >= head
    ? { kind: "latest", revision: held.revision }
    : { kind: "behind", revision: held.revision, head };
}
