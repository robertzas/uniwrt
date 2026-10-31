// The force layout off the main thread (ui.md, Performance budgets): requests in, positions out.
// Typed with the DOM's `self`, whose postMessage takes the same options as a worker's.
import { ForceRunner, type LayoutRequest } from "./force";

const runner = new ForceRunner((message) =>
  self.postMessage(message, { transfer: [message.positions.buffer] }),
);
self.onmessage = (event: MessageEvent<LayoutRequest>) => runner.handle(event.data);
