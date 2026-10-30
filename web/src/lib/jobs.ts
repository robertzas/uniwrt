import { createEffect, createStore, reconcile } from "solid-js";
import type { Job } from "../api/gen/Job";
import type { JobId } from "../api/gen/JobId";
import type { ServerMessage } from "../api/gen/ServerMessage";
import type { Live } from "./live";

/** The jobs the `jobs` topic reports: the latest ones at subscription, then every change. */
export interface JobsState {
  byId: Record<JobId, Job>;
  /** The first snapshot arrived. */
  ready: boolean;
}

/** Applies a `jobs` message to the state (a store draft or a plain object). */
export function applyJobs(state: JobsState, message: ServerMessage) {
  if (message.type !== "jobs_snapshot" && message.type !== "jobs_delta") return;
  for (const job of message.jobs) {
    const known = state.byId[job.id];
    if (known) reconcile(job)(known);
    else state.byId[job.id] = job;
  }
  if (message.type === "jobs_snapshot") state.ready = true;
}

/** Adds jobs a request answered with, unless the topic has newer news of them. */
export function addJobs(state: JobsState, jobs: readonly Job[]) {
  for (const job of jobs) {
    if (!state.byId[job.id]) state.byId[job.id] = job;
  }
}

/** The `jobs` topic for as long as the calling component lives. */
export function createJobsView(live: Live): [JobsState, (jobs: readonly Job[]) => void] {
  const [jobs, setJobs] = createStore<JobsState>({ byId: {}, ready: false });
  createEffect(
    () => live.socket,
    (socket) =>
      socket.subscribe("jobs", (message) => {
        setJobs((draft) => applyJobs(draft, message));
      }),
  );
  return [jobs, (added) => setJobs((draft) => addJobs(draft, added))];
}

/** The job target of root at `host` (`root@host:port`, an IPv6 address in brackets). */
export function sshTarget(host: string, port = 22): string {
  return `root@${host.includes(":") ? `[${host}]` : host}:${port}`;
}

/** The running adoption of `host`, found by its target. */
export function adoptionOf(state: JobsState, host: string, port = 22): Job | undefined {
  const target = sshTarget(host, port);
  return Object.values(state.byId).find(
    (job) => job.kind === "adopt" && job.state === "running" && job.target === target,
  );
}
