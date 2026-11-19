import { RotateCw } from "lucide";
import { Show } from "solid-js";
import type { Job } from "../../api/gen/Job";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { JobNodes } from "../../ui/JobNodes";
import { JobErrorNote, JobSuccessNote } from "../../ui/JobOutcome";
import { JobSteps, jobProgress } from "../../ui/JobSteps";

const t = strings.changes.progress;

/** An apply as it runs (the `jobs` topic): its steps, each node's state, then how it ended. */
export function ApplyProgress(props: { job: Job; onAgain: () => void }) {
  const running = () => props.job.state === "running";
  const revision = () => props.job.revision ?? 0;
  const percent = () => (running() ? Math.round(jobProgress(props.job) * 100) : 100);
  return (
    <Card aria-label={t.title(revision())}>
      <CardHeader
        title={t.title(revision())}
        description={running() ? t.running : undefined}
        actions={
          <Show when={!running()}>
            <Button size="sm" icon={RotateCw} onClick={() => props.onAgain()}>
              {strings.changes.again}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-4 px-4 pb-4">
        <div
          role="progressbar"
          aria-label={t.title(revision())}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent()}
          class="h-1.5 overflow-hidden rounded-full bg-raised"
        >
          <div
            class={[
              "h-full rounded-full transition-[width] duration-500",
              props.job.state === "failed" ? "bg-crit" : "bg-accent",
            ]}
            style={{ width: `${percent()}%` }}
          />
        </div>
        <Show when={props.job.state === "succeeded"}>
          <JobSuccessNote text={t.succeeded(revision())} />
        </Show>
        <Show when={props.job.error}>
          {(error) => (
            <div class="flex flex-col gap-2">
              <JobErrorNote error={error()} />
              <Show when={error().code === "partial_apply" || error().code === "rolled_back"}>
                <a
                  href="/settings?tab=revisions"
                  class="self-start text-accent text-sm hover:underline"
                >
                  {t.history}
                </a>
              </Show>
            </div>
          )}
        </Show>
        <div class="grid gap-4 md:grid-cols-2">
          <div class="rounded-control border border-edge p-2">
            <JobSteps job={props.job} />
          </div>
          <JobNodes
            nodes={props.job.nodes}
            label={t.devices}
            states={t.state}
            running={running()}
          />
        </div>
      </div>
    </Card>
  );
}
