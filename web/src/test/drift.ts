import type { Drift } from "../api/gen/Drift";

/** A drift of `pkg` on `node`: by default the system log's size, changed a minute ago. */
export function drift(node: number, pkg: string, extra: Partial<Drift> = {}): Drift {
  const now = Date.now();
  return {
    node,
    package: pkg,
    revision: 2,
    detected_at: now - 50_000,
    changed_at: now - 60_000,
    removed: false,
    diff: {
      package: pkg,
      sections: [
        {
          before: { type: "system", index: 0 },
          after: { type: "system", index: 0 },
          moved: false,
          options: [{ name: "log_size", before: "128", after: "64" }],
        },
      ],
    },
    ...extra,
  };
}
