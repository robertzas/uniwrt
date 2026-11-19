import type { Job } from "../../../api/gen/Job";
import type { NodeSummary } from "../../../api/gen/NodeSummary";
import type { JobsState } from "../../../lib/jobs";

/** As the picker lists them: the gateway last (where the job puts it), the others by name. */
export function rebootOrder(nodes: readonly NodeSummary[]): NodeSummary[] {
  return [...nodes].sort(
    (a, b) =>
      Number(a.role === "gateway") - Number(b.role === "gateway") || a.name.localeCompare(b.name),
  );
}

/** Only managed nodes restart (the API answers 409 for one in observe mode). */
export function canReboot(node: NodeSummary): boolean {
  return node.mode === "managed";
}

/** Picked at first: the managed nodes that answer (an offline one would stop the job). */
export function defaultPick(nodes: readonly NodeSummary[]): number[] {
  return nodes
    .filter((node) => canReboot(node) && node.status.state !== "offline")
    .map((node) => node.id);
}

/** A restart that runs on any of `ids`, started here or elsewhere. */
export function runningReboot(jobs: JobsState, ids: readonly number[]): Job | undefined {
  return Object.values(jobs.byId).find(
    (job) =>
      job.kind === "reboot" &&
      job.state === "running" &&
      job.nodes.some((entry) => ids.includes(entry.node)),
  );
}
