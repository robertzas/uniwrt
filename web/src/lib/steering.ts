/**
 * Client steering on the client and device pages (wifi.md, Steering & balancing: Timeline UI):
 * the `client_steered` events of a client or an access point over a span up to now, read
 * again each minute, and what each action came to.
 */
import { useQuery } from "@tanstack/solid-query";
import { createSignal, onSettled } from "solid-js";
import type { EventRecord } from "../api/gen/EventRecord";
import type { SteerAction } from "../api/gen/SteerAction";
import { api } from "./api";

const MINUTE_MS = 60_000;
/** Refusals of one client at one access point fold, so even a busy week stays under this. */
const LIMIT = 200;

export type Steered = EventRecord & {
  what: Extract<EventRecord["what"], { kind: "client_steered" }>;
};

/** What a steering action came to: the client moved, declined, was turned away, or cut off. */
export type SteerOutcome = "moved" | "declined" | "refused" | "kicked";

export const OUTCOMES: readonly SteerOutcome[] = ["moved", "declined", "refused", "kicked"];

export const outcomeOf: Record<SteerAction, SteerOutcome> = {
  transition_accepted: "moved",
  transition_rejected: "declined",
  probe_denied: "refused",
  auth_denied: "refused",
  assoc_denied: "refused",
  signal_kick: "kicked",
  load_kick: "kicked",
};

/** Each outcome's mark: a move is good, a refusal a warning, a disconnect worse. */
export const outcomeTones: Record<SteerOutcome, string> = {
  moved: "bg-ok",
  declined: "bg-fg-subtle",
  refused: "bg-warn",
  kicked: "bg-crit",
};

export function isSteered(event: EventRecord): event is Steered {
  return event.what.kind === "client_steered";
}

/** How many events came to each outcome, in [`OUTCOMES`]' order, those with none left out. */
export function outcomeCounts(events: readonly Steered[]): [SteerOutcome, number][] {
  const counts = new Map<SteerOutcome, number>();
  for (const event of events) {
    const outcome = outcomeOf[event.what.action];
    counts.set(outcome, (counts.get(outcome) ?? 0) + event.count);
  }
  return OUTCOMES.flatMap((outcome) => {
    const n = counts.get(outcome);
    return n ? [[outcome, n] as [SteerOutcome, number]] : [];
  });
}

/** Where to read steering: the nodes' or the clients' (either may be empty), over `spanMs`. */
export interface SteeringScope {
  nodes: number[];
  clients: string[];
  spanMs: number;
}

/**
 * The steering events in `scope` up to now, newest first: read on mount, again each minute
 * and whenever the scope changes (the previous list shown meanwhile); empty until read or when
 * reading fails.
 */
export function createSteering(scope: () => SteeringScope): () => Steered[] {
  const [minute, setMinute] = createSignal(Math.floor(Date.now() / MINUTE_MS));
  onSettled(() => {
    const timer = setInterval(() => setMinute(Math.floor(Date.now() / MINUTE_MS)), MINUTE_MS);
    return () => clearInterval(timer);
  });
  const query = useQuery(() => {
    const { nodes, clients, spanMs } = scope();
    const at = minute();
    return {
      queryKey: ["steering", nodes, clients, spanMs, at],
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        api.events.page(
          {
            categories: [],
            kinds: ["client_steered"],
            min: null,
            nodes,
            clients,
            from: (at + 1) * MINUTE_MS - spanMs,
            to: null,
          },
          null,
          LIMIT,
          signal,
        ),
      placeholderData: <T>(previous: T) => previous,
    };
  });
  return () => (query.data?.events ?? []).filter(isSteered);
}
