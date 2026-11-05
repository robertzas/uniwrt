import { describe, expect, it } from "vitest";
import { revisionMeta } from "../test/site";
import { applyConfig, type ConfigState, holding } from "./config";

function state(): ConfigState {
  return { head: null, applied: {}, ready: false };
}

describe("config", () => {
  it("takes each message whole", () => {
    const config = state();
    applyConfig(config, { type: "fleet_snapshot", topic: "fleet", nodes: [] });
    expect(config.ready).toBe(false);
    applyConfig(config, {
      type: "config",
      topic: "config",
      head: revisionMeta(3),
      applied: [
        { node: 1, revision: 2, job: 7, at: 0 },
        { node: 2, revision: 3, job: 8, at: 0 },
      ],
    });
    expect(config.ready).toBe(true);
    expect(config.head?.id).toBe(3);
    expect(Object.keys(config.applied)).toEqual(["1", "2"]);
    applyConfig(config, { type: "config", topic: "config", head: revisionMeta(4), applied: [] });
    expect(config.applied).toEqual({});
  });

  it("says where a node stands against the latest revision", () => {
    const config = state();
    applyConfig(config, {
      type: "config",
      topic: "config",
      head: revisionMeta(3),
      applied: [
        { node: 1, revision: 2, job: 7, at: 0 },
        { node: 2, revision: 3, job: 8, at: 0 },
      ],
    });
    expect(holding(config, 1, "observe")).toEqual({ kind: "observed" });
    expect(holding(config, 1, "managed")).toEqual({ kind: "behind", revision: 2, head: 3 });
    expect(holding(config, 2, "managed")).toEqual({ kind: "latest", revision: 3 });
    expect(holding(config, 3, "managed")).toEqual({ kind: "never" });
  });
});
