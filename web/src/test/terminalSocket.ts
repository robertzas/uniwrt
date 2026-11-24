import { vi } from "vitest";

/** A terminal's WebSocket the test drives: binary and text both ways (features/devices/terminal). */
export class TerminalSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: TerminalSocket[] = [];

  readyState = TerminalSocket.CONNECTING;
  binaryType = "blob";
  /** Text messages, parsed. */
  readonly texts: unknown[] = [];
  /** Binary messages, as bytes. */
  readonly bytes: Uint8Array[] = [];
  closedWith: number | undefined;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: ((event: { code: number }) => void) | null = null;

  constructor(readonly url: string) {
    TerminalSocket.instances.push(this);
  }

  static get last(): TerminalSocket {
    const ws = TerminalSocket.instances.at(-1);
    if (!ws) throw new Error("no terminal was opened");
    return ws;
  }

  send(data: string | Uint8Array) {
    if (typeof data === "string") this.texts.push(JSON.parse(data));
    else this.bytes.push(new Uint8Array(data));
  }

  close(code = 1000) {
    if (this.readyState === TerminalSocket.CLOSED) return;
    this.readyState = TerminalSocket.CLOSED;
    this.closedWith = code;
    this.onclose?.({ code });
  }

  open() {
    this.readyState = TerminalSocket.OPEN;
  }

  /** The controller sends JSON. */
  text(message: unknown) {
    this.onmessage?.({ data: JSON.stringify(message) });
  }

  /** The controller sends output. */
  output(text: string) {
    this.onmessage?.({ data: new TextEncoder().encode(text).buffer });
  }

  /** The controller (or the network) closes the socket. */
  drop(code = 1006) {
    this.readyState = TerminalSocket.CLOSED;
    this.onclose?.({ code });
  }
}

/** Replaces the global WebSocket with the terminal's fake; vitest restores it after the test. */
export function stubTerminalSocket() {
  TerminalSocket.instances = [];
  vi.stubGlobal("WebSocket", TerminalSocket);
}
