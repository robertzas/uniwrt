import type { JSX } from "@solidjs/web";
import { untrack } from "solid-js";
import type { NodeSummary } from "../api/gen/NodeSummary";
import { createLive, LiveContext } from "../lib/live";
import { type Session, SessionContext } from "../lib/session";
import { LiveSocket } from "../lib/ws";
import { FakeWebSocket, stubWebSocket } from "./socket";

/** A live socket over the fake WebSocket, already open, delivering messages at once. */
export function openTestSocket(): { socket: LiveSocket; ws: FakeWebSocket } {
  stubWebSocket();
  const socket = new LiveSocket({ url: "ws://test/api/v1/ws", schedule: (flush) => flush() });
  const ws = FakeWebSocket.last;
  ws.open();
  return { socket, ws };
}

/** Sends the fleet's snapshot as the hub would. */
export function sendFleet(ws: FakeWebSocket, nodes: NodeSummary[]) {
  ws.receive({ type: "fleet_snapshot", topic: "fleet", nodes });
}

/** What the signed-in app provides: a session for `admin` and the live data over `socket`. */
export function SignedInProviders(props: {
  socket: LiveSocket;
  signOut?: () => Promise<void>;
  children: JSX.Element;
}) {
  const session: Session = {
    user: { id: 1, username: "admin", role: "admin" },
    signOut: () => props.signOut?.() ?? Promise.resolve(),
  };
  const live = createLive(untrack(() => props.socket));
  return (
    <SessionContext value={session}>
      <LiveContext value={live}>{props.children}</LiveContext>
    </SessionContext>
  );
}
