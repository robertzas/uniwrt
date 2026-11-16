import type { ControllerUpdate } from "../api/gen/ControllerUpdate";

/** `GET v1/controller/update`: running 1.2.3, 1.2.4 on offer, nothing under way. */
export const controllerUpdate = (extra: Partial<ControllerUpdate> = {}): ControllerUpdate => ({
  current: "1.2.3",
  offer: {
    version: "1.2.4",
    name: "v1.2.4",
    notes: "https://github.com/robertzas/uniwrt/releases/tag/v1.2.4",
    published_at: Date.UTC(2026, 10, 15),
    size_bytes: 15 * 1024 * 1024,
  },
  note: null,
  blocked: null,
  progress: { state: "idle" },
  failure: null,
  settings: { auto: false, at_min: 150 },
  ...extra,
});
