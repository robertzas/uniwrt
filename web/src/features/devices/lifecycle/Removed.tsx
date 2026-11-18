import type { JSX } from "@solidjs/web";
import { createMemo, Show, useContext } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { NodeId } from "../../../api/gen/NodeId";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Card } from "../../../ui/Card";
import { JobErrorNote, JobSuccessNote } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";

const l = strings.device.lifecycle.removed;

/** The latest decommission or forget of `node` among `jobs`, if any (node ids are never reused). */
export function removalOf(jobs: readonly Job[], node: NodeId): Job | undefined {
  let latest: Job | undefined;
  for (const job of jobs) {
    if (job.node !== node || (job.kind !== "decommission" && job.kind !== "forget")) continue;
    if (!latest || job.id > latest.id) latest = job;
  }
  return latest;
}

/**
 * A device page whose node is gone: when a decommission or forget removed it, that job and its
 * outcome (it may still be finishing as the record goes), else `fallback`.
 */
export function Removed(props: { node: NodeId; fallback: JSX.Element }) {
  const live = useContext(LiveContext);
  const [jobs] = createJobsView(live);
  const job = createMemo(() => removalOf(Object.values(jobs.byId), props.node));
  return (
    <Show when={job()} fallback={<Show when={jobs.ready}>{props.fallback}</Show>}>
      {(job) => {
        const kind = () => (job().kind === "forget" ? "forget" : "decommission");
        return (
          <Card class="mx-auto flex max-w-xl flex-col gap-4 p-5">
            <h1 class="font-semibold text-xl tracking-tight">
              {job().state === "succeeded"
                ? l.done(job().target, kind())
                : strings.job.title[job().kind](job().target)}
            </h1>
            <JobSteps job={job()} />
            <Show when={job().state === "succeeded"}>
              <JobSuccessNote text={l.body} />
            </Show>
            <Show when={job().error}>{(failure) => <JobErrorNote error={failure()} />}</Show>
          </Card>
        );
      }}
    </Show>
  );
}
