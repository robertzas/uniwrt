/**
 * A device's packet capture as it runs (D-142): one per device, kept apart from the page, so that
 * it goes on while the admin looks at other tabs, and saved as a `.pcap` when it ends or is
 * stopped (cut to the last whole packet).
 */
import { type Accessor, createSignal } from "solid-js";
import type { CaptureQuery } from "../../../api/gen/CaptureQuery";
import type { NodeId } from "../../../api/gen/NodeId";
import { api, errorText } from "../../../lib/api";
import { saveFile } from "../../../lib/download";
import { PcapCounter } from "./capture";

const PCAP = "application/vnd.tcpdump.pcap";

export type CaptureState =
  | { phase: "starting" }
  | {
      phase: "capturing";
      name: string;
      started: number;
      seconds: number;
      maxBytes: number;
      bytes: number;
      packets: number;
    }
  /** Saved as `name`; `error` when it broke off rather than ended. */
  | { phase: "saved"; name: string; bytes: number; packets: number; error?: string }
  /** It ended without a packet: nothing to save. */
  | { phase: "empty" }
  | { phase: "failed"; error: string };

export interface CaptureSession {
  state: Accessor<CaptureState | undefined>;
  start: (query: CaptureQuery) => Promise<void>;
  /** Stops it: before it runs, nothing is saved; while it runs, what arrived is. */
  stop: () => void;
  /** Forgets how the last one ended. */
  clear: () => void;
}

const sessions = new Map<NodeId, CaptureSession>();

/** The device's capture session, made on first use. */
export function captureSession(node: NodeId): CaptureSession {
  const existing = sessions.get(node);
  if (existing) return existing;
  const [state, setState] = createSignal<CaptureState | undefined>(undefined);
  let stop: (() => void) | undefined;
  const busy = () => {
    const phase = state()?.phase;
    return phase === "starting" || phase === "capturing";
  };

  const start = async (query: CaptureQuery) => {
    if (busy()) return;
    const abort = new AbortController();
    stop = () => abort.abort();
    setState({ phase: "starting" });
    let opened: Awaited<ReturnType<typeof api.diag.capture>>;
    try {
      opened = await api.diag.capture(node, query, abort.signal);
    } catch (err) {
      stop = undefined;
      setState(abort.signal.aborted ? undefined : { phase: "failed", error: errorText(err) ?? "" });
      return;
    }
    const reader = opened.response.body?.getReader();
    const counter = new PcapCounter();
    const parts: Uint8Array[] = [];
    let bytes = 0;
    const base = {
      phase: "capturing" as const,
      name: opened.name,
      started: Date.now(),
      seconds: query.seconds,
      maxBytes: query.max_mib * 2 ** 20,
    };
    setState({ ...base, bytes, packets: 0 });
    stop = () => void reader?.cancel();
    let error: string | undefined;
    try {
      for (;;) {
        const chunk = await reader?.read();
        if (!chunk || chunk.done) break;
        parts.push(chunk.value);
        bytes += chunk.value.length;
        counter.push(chunk.value);
        setState({ ...base, bytes, packets: counter.packets });
      }
    } catch (err) {
      error = errorText(err) ?? String(err);
    }
    stop = undefined;
    if (counter.packets === 0) {
      setState(error ? { phase: "failed", error } : { phase: "empty" });
      return;
    }
    const kept = counter.complete;
    saveFile({
      blob: new Blob(parts as BlobPart[], { type: PCAP }).slice(0, kept, PCAP),
      name: opened.name,
    });
    setState({
      phase: "saved",
      name: opened.name,
      bytes: kept,
      packets: counter.packets,
      ...(error !== undefined && { error }),
    });
  };

  const session: CaptureSession = {
    state,
    start,
    stop: () => stop?.(),
    clear: () => {
      if (!busy()) setState(undefined);
    },
  };
  sessions.set(node, session);
  return session;
}
