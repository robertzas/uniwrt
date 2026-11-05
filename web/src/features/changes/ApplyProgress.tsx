import {
  Circle,
  CircleCheck,
  CircleCheckBig,
  CircleDot,
  CircleMinus,
  CircleX,
  LoaderCircle,
  RotateCw,
  Undo2,
} from "lucide";
import { For, Show } from "solid-js";
import type { Job } from "../../api/gen/Job";
import type { JobNodeState } from "../../api/gen/JobNodeState";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon, type IconNode } from "../../ui/Icon";
import { JobErrorNote, JobSuccessNote } from "../../ui/JobOutcome";
import { JobSteps, jobProgress } from "../../ui/JobSteps";

const t = strings.changes.progress;

const NODE_ICONS: Record<JobNodeState, { icon: IconNode; class: string }> = {
  pending: { icon: Circle, class: "text-fg-subtle" },
  unchanged: { icon: CircleMinus, class: "text-fg-subtle" },
  skipped: { icon: CircleMinus, class: "text-fg-subtle" },
  staged: { icon: CircleDot, class: "text-info" },
  applied: { icon: LoaderCircle, class: "animate-spin text-accent" },
  verified: { icon: CircleCheck, class: "text-accent" },
  confirmed: { icon: CircleCheckBig, class: "text-ok" },
  rolled_back: { icon: Undo2, class: "text-warn" },
  failed: { icon: CircleX, class: "text-crit" },
};

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
          <div class="rounded-control border border-edge">
            <h3 class="border-edge/60 border-b px-3 py-2 font-medium text-fg-muted text-xs">
              {t.devices}
            </h3>
            <ul aria-label={t.devices} class="flex flex-col">
              <For each={props.job.nodes}>
                {(node) => (
                  <li class="flex items-start gap-2.5 border-edge/40 border-b px-3 py-2 last:border-b-0">
                    <Icon
                      icon={NODE_ICONS[node.state].icon}
                      class={[
                        "mt-0.5 size-4 shrink-0",
                        NODE_ICONS[node.state].class,
                        { "animate-none": !running() },
                      ]}
                    />
                    <div class="min-w-0 flex-1">
                      <p class="flex flex-wrap items-baseline justify-between gap-x-2 text-sm">
                        <span class="font-medium text-fg">{node.name}</span>
                        <span class="text-fg-muted text-xs">{t.state[node.state]}</span>
                      </p>
                      <Show
                        when={
                          node.detail?.toLowerCase() !== t.state[node.state].toLowerCase() &&
                          node.detail
                        }
                      >
                        {(detail) => (
                          <p class="mt-0.5 break-words text-fg-subtle text-xs">{detail()}</p>
                        )}
                      </Show>
                    </div>
                  </li>
                )}
              </For>
            </ul>
          </div>
        </div>
      </div>
    </Card>
  );
}
