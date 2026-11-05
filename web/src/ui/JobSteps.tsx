import { Circle, CircleCheck, CircleMinus, CircleX, LoaderCircle } from "lucide";
import { For, Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon, type IconNode } from "./Icon";

const s = strings.job;

type StepState = "pending" | "running" | "done" | "skipped" | "failed";

/** A job as its checklist needs it (structurally the API's `Job`; ui/ imports no API types). */
export interface StepsJob {
  kind: keyof typeof s.title;
  target: string;
  state: "running" | "succeeded" | "failed";
  steps: readonly { name: keyof typeof s.step; state: StepState; detail: string | null }[];
}

const stepIcons: Record<StepState, { icon: IconNode; class: string }> = {
  pending: { icon: Circle, class: "text-fg-subtle" },
  running: { icon: LoaderCircle, class: "animate-spin text-accent" },
  done: { icon: CircleCheck, class: "text-ok" },
  skipped: { icon: CircleMinus, class: "text-fg-subtle" },
  failed: { icon: CircleX, class: "text-crit" },
};

/** A job's steps as a checklist: what each did, the running one spinning. */
export function JobSteps(props: { job: StepsJob }) {
  return (
    <ol class="flex flex-col gap-0.5" aria-label={s.title[props.job.kind](props.job.target)}>
      <For each={props.job.steps}>
        {(step) => (
          <li
            class={[
              "flex items-start gap-2.5 rounded-control px-2 py-1.5",
              { "bg-raised": step.state === "running" },
            ]}
          >
            <Icon
              icon={stepIcons[step.state].icon}
              class={["mt-0.5 size-4 shrink-0", stepIcons[step.state].class]}
            />
            <div class="min-w-0 flex-1">
              <p
                class={[
                  "text-sm",
                  step.state === "pending" || step.state === "skipped"
                    ? "text-fg-muted"
                    : "text-fg",
                ]}
              >
                {s.stepOf[props.job.kind]?.[step.name] ?? s.step[step.name]}
                <span class="sr-only">: {s.stepState[step.state]}</span>
              </p>
              <Show when={step.detail}>
                {(detail) => <p class="mt-0.5 break-words text-fg-subtle text-xs">{detail()}</p>}
              </Show>
            </div>
          </li>
        )}
      </For>
    </ol>
  );
}

/** How far a job got, from 0 to 1: finished steps count whole, the running one half. */
export function jobProgress(job: StepsJob): number {
  if (job.steps.length === 0) return job.state === "running" ? 0 : 1;
  const done = job.steps.reduce(
    (sum, step) => sum + (step.state === "running" ? 0.5 : step.state === "pending" ? 0 : 1),
    0,
  );
  return done / job.steps.length;
}

/** The step a job is on, or the one it failed at. */
export function currentStep(job: StepsJob) {
  return (
    job.steps.find((step) => step.state === "failed") ??
    job.steps.find((step) => step.state === "running") ??
    job.steps.find((step) => step.state === "pending")
  );
}
