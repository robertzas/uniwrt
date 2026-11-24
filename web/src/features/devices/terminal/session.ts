/**
 * A device's terminal as it runs (D-141): one per device, kept apart from the tab, so that it goes
 * on while the admin looks at the device's other tabs, and ended when the device page is left
 * or turns to another device ([`scopeTerminal`]).
 */
import { type Accessor, createEffect, createSignal, onCleanup } from "solid-js";
import type { NodeId } from "../../../api/gen/NodeId";
import { endSession, errorText } from "../../../lib/api";
import { describeEnd, TerminalConnection, type TerminalEnd, terminalUrl } from "./connection";
import type { XtermHandle } from "./xterm";

export type TerminalState =
  | { phase: "idle" }
  /** xterm.js is loading (the first time only). */
  | { phase: "loading" }
  | { phase: "connecting" }
  | { phase: "open" }
  | { phase: "ended"; end: TerminalEnd }
  /** xterm.js didn't load. */
  | { phase: "failed"; error: string };

export interface TerminalSession {
  state: Accessor<TerminalState>;
  /** Shows the terminal in `container` (the tab's), which must be in the document. */
  attach: (container: HTMLElement) => void;
  detach: () => void;
  /** Opens the shell, or opens it again after an end; the output so far stays above. */
  connect: () => Promise<void>;
  disconnect: () => void;
  focus: () => void;
}

const sessions = new Map<NodeId, Session>();

/** The device's terminal, made on first use. */
export function terminalSession(node: NodeId): TerminalSession {
  let session = sessions.get(node);
  if (!session) {
    session = new Session(node);
    sessions.set(node, session);
  }
  return session;
}

/** Ends the device's terminal, if it has one. */
export function closeTerminal(node: NodeId) {
  sessions.get(node)?.dispose();
  sessions.delete(node);
}

/** Ties the device's terminal to the device page: it ends when the page shows another or goes. */
export function scopeTerminal(node: Accessor<NodeId>) {
  let current: NodeId | undefined;
  createEffect(node, (id) => {
    if (current !== undefined && current !== id) closeTerminal(current);
    current = id;
  });
  onCleanup(() => {
    if (current !== undefined) closeTerminal(current);
  });
}

class Session implements TerminalSession {
  readonly state: Accessor<TerminalState>;
  readonly #setState: (state: TerminalState) => void;
  readonly #node: NodeId;
  readonly #host = document.createElement("div");
  #term: Promise<XtermHandle> | undefined;
  #handle: XtermHandle | undefined;
  #connection: TerminalConnection | undefined;
  #observer: ResizeObserver | undefined;
  #frame = 0;
  #disposed = false;

  constructor(node: NodeId) {
    this.#node = node;
    this.#host.className = "size-full";
    const [state, setState] = createSignal<TerminalState>({ phase: "idle" });
    this.state = state;
    this.#setState = (next) => setState(() => next);
  }

  attach(container: HTMLElement) {
    container.append(this.#host);
    if (typeof ResizeObserver !== "undefined") {
      this.#observer?.disconnect();
      this.#observer = new ResizeObserver(() => this.#refit());
      this.#observer.observe(container);
    }
    this.#refit();
  }

  detach() {
    this.#observer?.disconnect();
    this.#observer = undefined;
    cancelAnimationFrame(this.#frame);
    this.#host.remove();
  }

  async connect() {
    const phase = this.state().phase;
    if (phase === "loading" || phase === "connecting" || phase === "open") return;
    if (!this.#handle) this.#setState({ phase: "loading" });
    let term: XtermHandle;
    try {
      term = await this.#terminal();
    } catch (error) {
      this.#term = undefined;
      this.#setState({ phase: "failed", error: errorText(error) ?? String(error) });
      return;
    }
    if (this.#disposed) return;
    const { cols, rows } = term.fit();
    this.#setState({ phase: "connecting" });
    const connection: TerminalConnection = new TerminalConnection(
      terminalUrl(this.#node, cols, rows),
      {
        opened: () => {
          if (this.#connection !== connection) return;
          this.#setState({ phase: "open" });
          // The size may have changed while it opened.
          const size = term.fit();
          connection.resize(size.cols, size.rows);
          term.focus();
        },
        output: (bytes) => term.write(bytes),
        ended: (end) => {
          if (this.#connection !== connection) return;
          this.#connection = undefined;
          if (this.#disposed) return;
          term.write(`\r\n\x1b[2m[${describeEnd(end)}]\x1b[0m\r\n`);
          this.#setState({ phase: "ended", end });
          if (end.kind === "session") endSession();
        },
      },
      { cols, rows },
    );
    this.#connection = connection;
  }

  disconnect() {
    this.#connection?.close();
  }

  focus() {
    this.#handle?.focus();
  }

  dispose() {
    this.#disposed = true;
    this.disconnect();
    this.detach();
    this.#handle?.dispose();
  }

  #terminal(): Promise<XtermHandle> {
    this.#term ??= import("./xterm").then(async ({ createXterm }) => {
      const term = await createXterm(this.#host);
      if (this.#disposed) {
        term.dispose();
        throw new Error("closed");
      }
      term.onInput((bytes) => this.#connection?.send(bytes));
      term.onResize((cols, rows) => this.#connection?.resize(cols, rows));
      this.#handle = term;
      return term;
    });
    return this.#term;
  }

  /** Fits the terminal to its frame once per frame, however often the frame resizes. */
  #refit() {
    if (!this.#handle) return;
    cancelAnimationFrame(this.#frame);
    this.#frame = requestAnimationFrame(() => this.#handle?.fit());
  }
}
