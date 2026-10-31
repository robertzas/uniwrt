import type { LayoutPositions, LayoutRequest } from "./force";

/** The force layout as the page sees it, wherever it runs. */
export interface LayoutClient {
  send(request: LayoutRequest): void;
  dispose(): void;
}

/**
 * The layout in a Web Worker, or on the main thread where there are no workers (tests). The
 * main-thread runner loads d3-force on demand, so the page's chunk doesn't carry it twice.
 */
export function createLayoutClient(onPositions: (message: LayoutPositions) => void): LayoutClient {
  if (typeof Worker !== "undefined") {
    const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<LayoutPositions>) => onPositions(event.data);
    return {
      send: (request) => worker.postMessage(request),
      dispose: () => worker.terminate(),
    };
  }
  let queue: LayoutRequest[] | undefined = [];
  let runner: import("./force").ForceRunner | undefined;
  let disposed = false;
  void import("./force").then(({ ForceRunner }) => {
    if (disposed) return;
    runner = new ForceRunner(onPositions);
    for (const request of queue ?? []) runner.handle(request);
    queue = undefined;
  });
  return {
    send: (request) => {
      if (runner) runner.handle(request);
      else queue?.push(request);
    },
    dispose: () => {
      disposed = true;
      runner?.handle({ type: "stop" });
    },
  };
}
