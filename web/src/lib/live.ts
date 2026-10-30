import { createContext, createEffect, createStore, reconcile } from "solid-js";
import type { FactsPatch } from "../api/gen/FactsPatch";
import type { NodeDetail } from "../api/gen/NodeDetail";
import type { NodeFacts } from "../api/gen/NodeFacts";
import type { NodeId } from "../api/gen/NodeId";
import type { NodeRole } from "../api/gen/NodeRole";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { ServerMessage } from "../api/gen/ServerMessage";
import type { TimedEvent } from "../api/gen/TimedEvent";
import type { LiveSocket } from "./ws";

/** The adopted nodes as the `fleet` topic keeps them, in the order of `compareNodes`. */
export interface FleetState {
  nodes: NodeSummary[];
  /** The first snapshot arrived. */
  ready: boolean;
}

/** The live data of the signed-in app: the socket and the fleet it keeps current. */
export interface Live {
  socket: LiveSocket;
  fleet: FleetState;
}

/** Provided once signed in; features read the fleet and open node views through it. */
export const LiveContext = createContext<Live>();

const roleOrder: Record<NodeRole, number> = { gateway: 0, ap: 1, bridge: 2 };

/** The gateway first, then access points and bridges, each by name. */
export function compareNodes(a: NodeSummary, b: NodeSummary): number {
  return roleOrder[a.role] - roleOrder[b.role] || a.name.localeCompare(b.name) || a.id - b.id;
}

/**
 * Merges `nodes` into the list in place: a node already listed keeps its object, so rows re-render
 * only the cells that changed. (`reconcile` with a key replaces array items in this release.)
 */
function mergeNodes(
  state: FleetState,
  nodes: readonly NodeSummary[],
  keep: (id: NodeId) => boolean,
) {
  const incoming = new Map(nodes.map((node) => [node.id, node]));
  const next: NodeSummary[] = [];
  for (const node of state.nodes) {
    const update = incoming.get(node.id);
    if (update) {
      reconcile(update)(node);
      incoming.delete(node.id);
    }
    if (update || keep(node.id)) next.push(node);
  }
  next.push(...incoming.values());
  next.sort(compareNodes);
  if (next.length !== state.nodes.length || next.some((node, i) => node !== state.nodes[i])) {
    state.nodes.splice(0, state.nodes.length, ...next);
  }
}

/** Applies a `fleet` message to the state (a store draft or a plain object). */
export function applyFleet(state: FleetState, message: ServerMessage) {
  if (message.type === "fleet_snapshot") {
    mergeNodes(state, message.nodes, () => false);
    state.ready = true;
  } else if (message.type === "fleet_delta") {
    const removed = new Set(message.removed);
    mergeNodes(state, message.nodes, (id) => !removed.has(id));
  }
}

/** The fleet store over `socket`, subscribed to `fleet` for as long as the socket is open. */
export function createLive(socket: LiveSocket): Live {
  const [fleet, setFleet] = createStore<FleetState>({ nodes: [], ready: false });
  socket.subscribe("fleet", (message) => {
    setFleet((draft) => applyFleet(draft, message));
  });
  return { socket, fleet };
}

/** Samples kept for the overview's sparklines: 60, about 5 minutes at the focused interval. */
export const HISTORY = 60;
/** Events kept for the overview. */
const EVENTS = 100;

/** One node as the `node:<id>` topic keeps it. */
export interface NodeViewState {
  detail: NodeDetail | null;
  /** The first snapshot (or an error) arrived. */
  ready: boolean;
  /** The hub has no such node, or stopped watching it. */
  missing: boolean;
  /** Newest last. */
  events: TimedEvent[];
  /** CPU and memory use in percent, one sample per collection, newest last. */
  cpu: number[];
  memory: number[];
}

export function emptyNodeView(): NodeViewState {
  return { detail: null, ready: false, missing: false, events: [], cpu: [], memory: [] };
}

function push(values: number[], value: number | null | undefined) {
  if (value == null) return;
  values.push(value);
  if (values.length > HISTORY) values.splice(0, values.length - HISTORY);
}

function sample(state: NodeViewState, facts: NodeFacts) {
  const memory = facts.system.memory;
  push(state.cpu, facts.system.cpu_percent);
  if (memory.total_bytes > 0) {
    push(state.memory, (100 * (memory.total_bytes - memory.available_bytes)) / memory.total_bytes);
  }
}

/** The patch as whole facts, when it carries the parts that are not lists; else undefined. */
function factsFrom(patch: FactsPatch): NodeFacts | undefined {
  const { board, system, capabilities } = patch;
  if (!board || !system || !capabilities) return undefined;
  return {
    interfaces: [],
    devices: [],
    phys: [],
    radios: [],
    stations: [],
    leases: [],
    neighbors: [],
    fdb: [],
    batman: [],
    ...patch,
    board,
    system,
    capabilities,
  };
}

/**
 * Applies a `node:<id>` message to the state (a store draft or a plain object). Returns false when
 * a delta cannot be applied (facts patched before any arrived), so the caller asks for a snapshot.
 */
export function applyNode(state: NodeViewState, message: ServerMessage): boolean {
  switch (message.type) {
    case "node_snapshot":
      state.detail = message.node;
      state.ready = true;
      state.missing = false;
      if (message.node.facts) sample(state, message.node.facts);
      return true;
    case "error":
      state.ready = true;
      state.missing = true;
      return true;
    case "node_delta": {
      if (message.removed) state.missing = true;
      const detail = state.detail;
      if (!detail) return !message.status && !message.facts;
      if (message.status) detail.status = message.status;
      if (message.facts) {
        if (detail.facts) {
          const patch = message.facts;
          for (const key of Object.keys(patch) as (keyof FactsPatch)[]) {
            if (patch[key] !== undefined) Object.assign(detail.facts, { [key]: patch[key] });
          }
        } else {
          const facts = factsFrom(message.facts);
          if (!facts) return false;
          detail.facts = facts;
        }
        if (message.facts.system) sample(state, detail.facts);
      }
      if (message.events.length > 0) {
        state.events.push(...message.events);
        if (state.events.length > EVENTS) state.events.splice(0, state.events.length - EVENTS);
      }
      return true;
    }
    default:
      return true;
  }
}

/**
 * The live view of one node: subscribes to `node:<id>` while the calling component lives (which
 * makes the engine collect it more often, D-041) and follows `id` when it changes.
 */
export function createNodeView(live: Live, id: () => NodeId): NodeViewState {
  const [view, setView] = createStore<NodeViewState>(emptyNodeView());
  createEffect(id, (nodeId) => {
    const topic = `node:${nodeId}`;
    setView(() => emptyNodeView());
    const unsubscribe = live.socket.subscribe(topic, (message) => {
      let applied = true;
      setView((draft) => {
        applied = applyNode(draft, message);
      });
      if (!applied) live.socket.resync(topic);
    });
    return unsubscribe;
  });
  return view;
}
