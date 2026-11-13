import { TriangleAlert, X } from "lucide";
import { For, Match, Show, Switch } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { JobsState } from "../../../lib/jobs";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Icon } from "../../../ui/Icon";
import { JobSuccessNote } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";

const t = strings.ztp.bench;

/** An adoption started from the adoption port, and the device's name as the inbox showed it. */
export interface BenchDevice {
  job: JobId;
  name: string;
}

/** Whether `job` is one this list follows while it runs (its inbox row is hidden meanwhile). */
export function onBench(bench: readonly BenchDevice[], jobs: JobsState, job: JobId | null) {
  return job !== null && jobs.byId[job]?.state === "running" && bench.some((b) => b.job === job);
}

/**
 * The adoptions started from the adoption port (adoption.md, Path C; D-107), followed to their
 * end: their steps while they run (the inbox drops the device once it is recorded, minutes
 * before its configuration is applied), then "unplug it and place it", or why its configuration
 * wasn't applied. A failed one is left to its inbox row, which shows why.
 */
export function BenchJobs(props: {
  devices: readonly BenchDevice[];
  jobs: JobsState;
  onClose: (job: JobId) => void;
}) {
  const shown = () =>
    props.devices.flatMap((device) => {
      const job = props.jobs.byId[device.job];
      return job && job.state !== "failed" ? [{ device, job }] : [];
    });
  return (
    <Show when={shown().length > 0}>
      <section class="border-edge border-t px-4 py-3" aria-label={t.title}>
        <h3 class="font-medium text-sm">{t.title}</h3>
        <ul class="mt-2 flex flex-col gap-3">
          <For each={shown()} keyed={(entry) => entry.device.job}>
            {(entry) => (
              <Bench
                name={entry().device.name}
                job={entry().job}
                onClose={() => props.onClose(entry().device.job)}
              />
            )}
          </For>
        </ul>
      </section>
    </Show>
  );
}

function Bench(props: { name: string; job: Job; onClose: () => void }) {
  const configure = () => props.job.steps.find((step) => step.name === "configure");
  const close = () => (
    <Button size="sm" variant="ghost" icon={X} onClick={props.onClose}>
      {t.close}
    </Button>
  );
  return (
    <li>
      <Switch>
        <Match when={props.job.state === "running"}>
          <p class="mb-1 text-fg-muted text-sm">{t.adopting(props.name)}</p>
          <JobSteps job={props.job} />
        </Match>
        <Match when={configure()?.state === "done"}>
          <div class="flex items-start gap-2">
            <div class="min-w-0 flex-1">
              <JobSuccessNote text={t.placeIt(props.name)} />
            </div>
            {close()}
          </div>
        </Match>
        <Match when={true}>
          <div class="flex items-start gap-2">
            <div
              role="alert"
              class="flex min-w-0 flex-1 items-start gap-2.5 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm"
            >
              <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
              <div class="min-w-0">
                <p class="text-fg">{t.unconfigured(props.name)}</p>
                <Show when={configure()?.detail}>
                  {(detail) => (
                    <p class="mt-1 break-words font-mono text-fg-subtle text-xs">{detail()}</p>
                  )}
                </Show>
              </div>
            </div>
            {close()}
          </div>
        </Match>
      </Switch>
    </li>
  );
}
