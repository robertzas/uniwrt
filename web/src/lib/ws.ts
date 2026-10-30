import { type Accessor, createSignal } from "solid-js";
import type { ClientMessage } from "../api/gen/ClientMessage";
import type { ServerMessage } from "../api/gen/ServerMessage";

/** `connecting` until the first open, then `open`, or `reconnecting` after a drop. */
export type SocketState = "connecting" | "open" | "reconnecting";

export type MessageHandler = (message: ServerMessage) => void;

export interface LiveSocketOptions {
  /** Defaults to `/api/v1/ws` on this page's host. */
  url?: string;
  /** The server closed the socket with 4401: the session ended (D-041). */
  onSessionEnded?: () => void;
  /** Runs `flush` once before the next frame; tests pass a synchronous one. */
  schedule?: (flush: () => void) => void;
}

/** The close code the hub sends when the session behind a socket ends. */
export const SESSION_ENDED = 4401;
const NORMAL = 1000;
const MAX_BACKOFF_MS = 30_000;

/**
 * The next animation frame, or a second later at the latest: a hidden tab gets no frames, and its
 * messages must not pile up.
 */
function nextFrame(flush: () => void) {
  let done = false;
  const run = () => {
    if (done) return;
    done = true;
    cancelAnimationFrame(frame);
    clearTimeout(timer);
    flush();
  };
  const frame = requestAnimationFrame(run);
  const timer = setTimeout(run, 1000);
}

function defaultUrl() {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${location.host}/api/v1/ws`;
}

/**
 * The one WebSocket to the hub (D-041). Components subscribe to topics; the socket tells the hub
 * the whole set whenever it changes, reconnects with jittered backoff after a drop (the hub then
 * sends every snapshot again), and hands the messages that arrived to their topic's handlers once
 * per animation frame, so a burst of deltas costs one render.
 */
export class LiveSocket {
  readonly state: Accessor<SocketState>;
  readonly #setState: (state: SocketState) => void;
  readonly #url: string;
  readonly #options: LiveSocketOptions;
  readonly #schedule: (flush: () => void) => void;
  readonly #topics = new Map<string, Set<MessageHandler>>();
  #ws: WebSocket | undefined;
  #queue: ServerMessage[] = [];
  #flushing = false;
  #subPending = false;
  #attempt = 0;
  #retry: ReturnType<typeof setTimeout> | undefined;
  #closed = false;

  constructor(options: LiveSocketOptions = {}) {
    const [state, setState] = createSignal<SocketState>("connecting");
    this.state = state;
    this.#setState = setState;
    this.#options = options;
    this.#url = options.url ?? defaultUrl();
    this.#schedule = options.schedule ?? nextFrame;
    this.#connect();
  }

  /** Receives the topic's messages, a snapshot first. Returns the function that unsubscribes. */
  subscribe(topic: string, handler: MessageHandler): () => void {
    let handlers = this.#topics.get(topic);
    if (!handlers) {
      handlers = new Set();
      this.#topics.set(topic, handlers);
      this.#sendTopicsSoon();
    } else {
      // The hub sent the topic's snapshot before this handler came: ask again.
      this.resync(topic);
    }
    handlers.add(handler);
    return () => {
      const current = this.#topics.get(topic);
      if (!current?.delete(handler) || current.size > 0) return;
      this.#topics.delete(topic);
      this.#sendTopicsSoon();
    };
  }

  /** Asks the hub for a fresh snapshot of `topic`: a delta could not be applied. */
  resync(topic: string) {
    if (!this.#topics.has(topic) || this.#ws?.readyState !== WebSocket.OPEN) return;
    this.#send({ sub: [...this.#topics.keys()].filter((t) => t !== topic) });
    this.#send({ sub: [...this.#topics.keys()] });
  }

  close() {
    this.#closed = true;
    clearTimeout(this.#retry);
    this.#ws?.close(NORMAL);
    this.#ws = undefined;
  }

  #connect() {
    const ws = new WebSocket(this.#url);
    this.#ws = ws;
    ws.onopen = () => {
      this.#attempt = 0;
      this.#setState("open");
      this.#send({ sub: [...this.#topics.keys()] });
    };
    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      this.#queue.push(JSON.parse(event.data) as ServerMessage);
      if (!this.#flushing) {
        this.#flushing = true;
        this.#schedule(() => this.#flush());
      }
    };
    ws.onclose = (event) => {
      if (this.#ws !== ws || this.#closed) return;
      this.#ws = undefined;
      if (event.code === SESSION_ENDED) {
        this.#closed = true;
        this.#options.onSessionEnded?.();
        return;
      }
      this.#setState("reconnecting");
      // 1 s, 2 s, 4 s … up to 30 s, each between half and all of it.
      const backoff = Math.min(MAX_BACKOFF_MS, 1000 * 2 ** this.#attempt);
      this.#attempt += 1;
      this.#retry = setTimeout(() => this.#connect(), backoff * (0.5 + Math.random() / 2));
    };
  }

  #flush() {
    this.#flushing = false;
    const messages = this.#queue;
    this.#queue = [];
    for (const message of messages) {
      if (message.topic === null) continue;
      for (const handler of this.#topics.get(message.topic) ?? []) handler(message);
    }
  }

  #sendTopicsSoon() {
    if (this.#subPending) return;
    this.#subPending = true;
    queueMicrotask(() => {
      this.#subPending = false;
      if (this.#ws?.readyState === WebSocket.OPEN) this.#send({ sub: [...this.#topics.keys()] });
    });
  }

  #send(message: ClientMessage) {
    this.#ws?.send(JSON.stringify(message));
  }
}
