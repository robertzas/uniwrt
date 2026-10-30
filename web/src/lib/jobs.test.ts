import { createRoot, createStore, flush } from "solid-js";
import { describe, expect, it } from "vitest";
import { adoptJob } from "../test/fixtures";
import { addJobs, adoptionOf, applyJobs, type JobsState, sshTarget } from "./jobs";

describe("jobs", () => {
  it("keeps the snapshot, then merges deltas into the same objects", () => {
    const [state, setState, dispose] = createRoot(
      (dispose) => [...createStore<JobsState>({ byId: {}, ready: false }), dispose] as const,
    );
    setState((draft) =>
      applyJobs(draft, {
        type: "jobs_snapshot",
        topic: "jobs",
        jobs: [adoptJob(1, "10.0.0.2", 2)],
      }),
    );
    flush();
    expect(state.ready).toBe(true);
    const first = state.byId[1];
    setState((draft) =>
      applyJobs(draft, { type: "jobs_delta", topic: "jobs", jobs: [adoptJob(1, "10.0.0.2", 3)] }),
    );
    flush();
    expect(state.byId[1]).toBe(first);
    expect(state.byId[1]?.steps[3]?.state).toBe("running");
    dispose();
  });

  it("adds a started job only if the topic has not reported it", () => {
    const state: JobsState = { byId: {}, ready: true };
    applyJobs(state, { type: "jobs_delta", topic: "jobs", jobs: [adoptJob(1, "10.0.0.2", 4)] });
    addJobs(state, [adoptJob(1, "10.0.0.2", 0), adoptJob(2, "10.0.0.3", 0)]);
    expect(state.byId[1]?.steps[4]?.state).toBe("running");
    expect(state.byId[2]).toBeDefined();
  });

  it("finds a running adoption by its target", () => {
    const state: JobsState = { byId: {}, ready: true };
    addJobs(state, [
      adoptJob(1, "10.0.0.2", 1),
      adoptJob(2, "10.0.0.3", 8, "done", { state: "succeeded" }),
    ]);
    expect(adoptionOf(state, "10.0.0.2")?.id).toBe(1);
    expect(adoptionOf(state, "10.0.0.3")).toBeUndefined();
    expect(sshTarget("2001:db8::1", 2222)).toBe("root@[2001:db8::1]:2222");
  });
});
