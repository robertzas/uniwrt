import { describe, expect, it } from "vitest";
import { drift } from "../test/drift";
import { revisionMeta } from "../test/site";
import {
  applyConfig,
  type ConfigState,
  driftsOf,
  holding,
  lastApplied,
  mutesOf,
  pendingNodes,
  unacknowledged,
} from "./config";

function state(): ConfigState {
  return { head: null, applied: {}, drift: { drifts: [], muted: [] }, ready: false };
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

  it("keeps the drift state, empty where the server watches none", () => {
    const config = state();
    const acknowledged = drift(2, "wireless", { acknowledged: { at: 1, by: "ada" } });
    applyConfig(config, {
      type: "config",
      topic: "config",
      head: revisionMeta(3),
      applied: [],
      drift: {
        drifts: [drift(1, "system"), drift(1, "firewall"), acknowledged],
        muted: [{ node: 2, package: "dhcp", at: 1, by: "ada" }],
      },
    });
    expect(driftsOf(config, 1).map((d) => d.package)).toEqual(["system", "firewall"]);
    expect(mutesOf(config, 2).map((m) => m.package)).toEqual(["dhcp"]);
    expect(mutesOf(config, 1)).toEqual([]);
    expect(unacknowledged(config).map((d) => `${d.node}/${d.package}`)).toEqual([
      "1/system",
      "1/firewall",
    ]);
    applyConfig(config, { type: "config", topic: "config", head: revisionMeta(3), applied: [] });
    expect(config.drift).toEqual({ drifts: [], muted: [] });
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

  it("counts a restore of what a node holds as current, and finds what discard goes back to", () => {
    const config = state();
    const nodes = [
      { id: 1, mode: "managed" as const },
      { id: 2, mode: "managed" as const },
      { id: 3, mode: "observe" as const },
      { id: 4, mode: "managed" as const },
    ];
    expect(pendingNodes(config, nodes)).toEqual([]);
    applyConfig(config, {
      type: "config",
      topic: "config",
      head: revisionMeta(8),
      applied: [
        { node: 1, revision: 5, job: 7, at: 0 },
        { node: 2, revision: 4, job: 6, at: 0 },
        { node: 3, revision: 2, job: 3, at: 0 },
      ],
    });
    expect(pendingNodes(config, nodes).map((node) => node.id)).toEqual([1, 2, 4]);
    expect(lastApplied(config, nodes)).toBe(5);
    applyConfig(config, {
      type: "config",
      topic: "config",
      head: revisionMeta(9, { kind: "restore", restored_from: 5 }),
      applied: [
        { node: 1, revision: 5, job: 7, at: 0 },
        { node: 2, revision: 4, job: 6, at: 0 },
      ],
    });
    expect(holding(config, 1, "managed")).toEqual({ kind: "copy", revision: 5, head: 9 });
    expect(pendingNodes(config, nodes).map((node) => node.id)).toEqual([2, 4]);
    expect(lastApplied(config, nodes)).toBeUndefined();
    applyConfig(config, { type: "config", topic: "config", head: revisionMeta(9), applied: [] });
    expect(lastApplied(config, nodes)).toBeUndefined();
  });
});
