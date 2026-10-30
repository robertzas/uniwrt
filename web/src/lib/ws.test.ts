import { createRoot, flush } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ServerMessage } from "../api/gen/ServerMessage";
import { detail, facts } from "../test/fixtures";
import { FakeWebSocket, stubWebSocket } from "../test/socket";
import { createLive, createNodeView } from "./live";
import { LiveSocket, SESSION_ENDED } from "./ws";

const tick = () => new Promise<void>((resolve) => queueMicrotask(resolve));

describe("LiveSocket", () => {
  beforeEach(() => {
    stubWebSocket();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("sends the topic set on open and whenever it changes", async () => {
    const socket = new LiveSocket({ url: "ws://test/api/v1/ws", schedule: (f) => f() });
    const ws = FakeWebSocket.last;
    const off = socket.subscribe("fleet", () => {});
    ws.open();
    expect(ws.sent).toEqual([{ sub: ["fleet"] }]);

    socket.subscribe("node:1", () => {});
    off();
    await tick();
    expect(ws.sent.at(-1)).toEqual({ sub: ["node:1"] });
    socket.close();
  });

  it("delivers the messages of a frame together, to their topic only", () => {
    const frames: (() => void)[] = [];
    const socket = new LiveSocket({ url: "ws://test", schedule: (f) => frames.push(f) });
    const fleet: ServerMessage[] = [];
    const node: ServerMessage[] = [];
    socket.subscribe("fleet", (m) => fleet.push(m));
    socket.subscribe("node:1", (m) => node.push(m));
    const ws = FakeWebSocket.last;
    ws.open();
    ws.receive({ type: "fleet_snapshot", topic: "fleet", nodes: [] });
    ws.receive({ type: "fleet_delta", topic: "fleet", nodes: [], removed: [2] });
    ws.receive({ type: "error", topic: null, message: "bad message" });
    expect(frames).toHaveLength(1);
    expect(fleet).toHaveLength(0);

    frames[0]?.();
    expect(fleet.map((m) => m.type)).toEqual(["fleet_snapshot", "fleet_delta"]);
    expect(node).toHaveLength(0);
    socket.close();
  });

  it("asks again for the snapshot when a second handler joins a topic", () => {
    const socket = new LiveSocket({ url: "ws://test", schedule: (f) => f() });
    socket.subscribe("node:1", () => {});
    const ws = FakeWebSocket.last;
    ws.open();
    socket.subscribe("node:1", () => {});
    expect(ws.sent).toEqual([{ sub: ["node:1"] }, { sub: [] }, { sub: ["node:1"] }]);
    socket.close();
  });

  it("reconnects with backoff after a drop, and stops when the session ended", () => {
    vi.useFakeTimers();
    vi.spyOn(Math, "random").mockReturnValue(1);
    const ended = vi.fn();
    const socket = new LiveSocket({ url: "ws://test", onSessionEnded: ended });
    FakeWebSocket.last.open();
    FakeWebSocket.last.drop();
    flush();
    expect(socket.state()).toBe("reconnecting");

    vi.advanceTimersByTime(999);
    expect(FakeWebSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeWebSocket.instances).toHaveLength(2);
    FakeWebSocket.last.drop();
    vi.advanceTimersByTime(2000);
    expect(FakeWebSocket.instances).toHaveLength(3);

    FakeWebSocket.last.open();
    flush();
    expect(socket.state()).toBe("open");
    FakeWebSocket.last.drop(SESSION_ENDED);
    expect(ended).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(60_000);
    expect(FakeWebSocket.instances).toHaveLength(3);
  });

  it("does not reconnect once closed", () => {
    vi.useFakeTimers();
    const socket = new LiveSocket({ url: "ws://test" });
    socket.close();
    vi.advanceTimersByTime(60_000);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });
});

describe("createNodeView", () => {
  beforeEach(() => {
    stubWebSocket();
  });

  it("follows the node's topic and applies its messages", async () => {
    const socket = new LiveSocket({ url: "ws://test", schedule: (f) => f() });
    const ws = FakeWebSocket.last;
    ws.open();
    const { view, dispose } = createRoot((dispose) => {
      const live = createLive(socket);
      return { view: createNodeView(live, () => 1), dispose };
    });
    flush();
    await tick();
    expect(ws.sent.at(-1)).toEqual({ sub: ["fleet", "node:1"] });

    ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(facts(12)) });
    flush();
    expect(view.ready).toBe(true);
    expect(view.detail?.facts?.system.cpu_percent).toBe(12);

    // A patch that cannot apply asks for a fresh snapshot.
    ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(null) });
    ws.receive({
      type: "node_delta",
      topic: "node:1",
      facts: { collected_at: 2, system: facts(1).system },
      events: [],
      roams: [],
      removed: false,
    });
    expect(ws.sent.slice(-2)).toEqual([{ sub: ["fleet"] }, { sub: ["fleet", "node:1"] }]);

    dispose();
    await tick();
    expect(ws.sent.at(-1)).toEqual({ sub: ["fleet"] });
    socket.close();
  });
});
