import type { NodeId } from "../../../api/gen/NodeId";
import type { TerminalClientMessage } from "../../../api/gen/TerminalClientMessage";
import type { TerminalServerMessage } from "../../../api/gen/TerminalServerMessage";
import { strings } from "../../../lib/strings";
import { SESSION_ENDED } from "../../../lib/ws";

/** The largest message the controller takes (64 KiB): a longer paste goes in parts. */
export const PART = 64 * 1024;
const NORMAL = 1000;

/** How a terminal ended. */
export type TerminalEnd =
  /** The controller said why: the shell exited (with its code), the bounds, idling, a failure. */
  | { kind: "ended"; exitCode: number | null; reason: string | null }
  /** The admin disconnected. */
  | { kind: "closed" }
  /** The session behind it ended (close code 4401): sign in again. */
  | { kind: "session" }
  /** The socket closed without saying why (the network, the controller). */
  | { kind: "lost" };

export interface TerminalEvents {
  /** The shell runs. */
  opened: () => void;
  /** Bytes the terminal wrote. */
  output: (bytes: Uint8Array) => void;
  /** Called once, last. */
  ended: (end: TerminalEnd) => void;
}

/** The terminal's WebSocket on this page's host (features/observability.md, Terminal). */
export function terminalUrl(node: NodeId, cols: number, rows: number, at: Location = location) {
  const scheme = at.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${at.host}/api/v1/nodes/${node}/terminal?cols=${cols}&rows=${rows}`;
}

/** Words for how a terminal ended. */
export function describeEnd(end: TerminalEnd): string {
  const t = strings.device.terminal.end;
  switch (end.kind) {
    case "ended": {
      const why = end.reason ?? t.ended;
      return end.exitCode === null || end.exitCode === 0 ? why : t.withCode(why, end.exitCode);
    }
    case "closed":
      return t.closed;
    case "session":
      return t.session;
    case "lost":
      return t.lost;
  }
}

/**
 * One terminal's WebSocket (D-141): keystrokes go as binary messages, at most [`PART`] each;
 * output arrives as binary; text carries JSON, the browser's resize and the controller's
 * `opened` and `ended`. It is used once: a reconnect is another connection.
 */
export class TerminalConnection {
  readonly #ws: WebSocket;
  readonly #events: TerminalEvents;
  #said: TerminalEnd | undefined;
  #done = false;
  #size = "";

  /** `size` is the one the URL asked for. */
  constructor(url: string, events: TerminalEvents, size?: { cols: number; rows: number }) {
    this.#events = events;
    if (size) this.#size = `${size.cols}x${size.rows}`;
    this.#ws = new WebSocket(url);
    this.#ws.binaryType = "arraybuffer";
    this.#ws.onmessage = (event: MessageEvent) => this.#receive(event.data);
    this.#ws.onclose = (event: CloseEvent) =>
      this.#finish(
        event.code === SESSION_ENDED ? { kind: "session" } : (this.#said ?? { kind: "lost" }),
      );
  }

  /** Sends keystrokes (or a paste), in parts the controller takes. */
  send(bytes: Uint8Array<ArrayBuffer>) {
    if (this.#done || this.#ws.readyState !== WebSocket.OPEN) return;
    for (let at = 0; at < bytes.length; at += PART) {
      this.#ws.send(bytes.subarray(at, at + PART));
    }
  }

  /** Tells the controller the terminal's size, when it changed. */
  resize(cols: number, rows: number) {
    const size = `${cols}x${rows}`;
    if (this.#done || this.#ws.readyState !== WebSocket.OPEN || size === this.#size) return;
    this.#size = size;
    const message: TerminalClientMessage = { type: "resize", cols, rows };
    this.#ws.send(JSON.stringify(message));
  }

  /** Disconnects: the controller closes the SSH channel, and the shell is hung up. */
  close() {
    this.#finish({ kind: "closed" });
    this.#ws.close(NORMAL);
  }

  #receive(data: unknown) {
    if (this.#done) return;
    // Binary messages arrive as ArrayBuffers (binaryType).
    if (typeof data !== "string") {
      this.#events.output(new Uint8Array(data as ArrayBuffer));
      return;
    }
    let message: TerminalServerMessage;
    try {
      message = JSON.parse(data) as TerminalServerMessage;
    } catch {
      return;
    }
    if (message.type === "opened") this.#events.opened();
    else if (message.type === "ended") {
      this.#said = { kind: "ended", exitCode: message.exit_code, reason: message.reason };
    }
  }

  #finish(end: TerminalEnd) {
    if (this.#done) return;
    this.#done = true;
    this.#events.ended(end);
  }
}
