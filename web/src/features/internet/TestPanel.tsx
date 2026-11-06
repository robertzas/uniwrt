import { CircleCheck, TriangleAlert } from "lucide";
import { Match, Show, Switch } from "solid-js";
import type { Job } from "../../api/gen/Job";
import { strings } from "../../lib/strings";
import { Checkbox } from "../../ui/Checkbox";
import { Icon } from "../../ui/Icon";
import { JobErrorNote } from "../../ui/JobOutcome";
import { JobSteps } from "../../ui/JobSteps";
import { ipv6Result } from "./summary";

const w = strings.internet.wizard;

/**
 * A WAN test as it runs (wan.md, Connectivity test): its steps live, then what it found. When
 * IPv6 alone failed, the user may save with IPv6 off.
 */
export function TestPanel(props: {
  job: Job | undefined;
  ipv6Off: boolean;
  onIpv6Off: (off: boolean) => void;
}) {
  const ended = () => props.job !== undefined && props.job.state !== "running";
  return (
    <div class="flex flex-col gap-4">
      <Show when={props.job} fallback={<p class="text-fg-muted text-sm">{w.starting}</p>}>
        {(job) => (
          <div class="rounded-control border border-edge p-3" role="status" aria-live="polite">
            <JobSteps job={job()} />
          </div>
        )}
      </Show>
      <Show when={ended() && props.job}>
        {(job) => (
          <Switch>
            <Match when={job().state === "failed" && job().error}>
              {(error) => <JobErrorNote error={error()} />}
            </Match>
            <Match when={ipv6Result(job()) === "failed"}>
              <div class="flex flex-col gap-3 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
                <p class="flex items-start gap-2.5">
                  <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
                  {w.noIpv6}
                </p>
                <Checkbox checked={props.ipv6Off} onChange={props.onIpv6Off} label={w.ipv6Off} />
              </div>
            </Match>
            <Match when={true}>
              <p class="flex items-start gap-2.5 rounded-control border border-ok/40 bg-ok/10 px-3 py-2.5 text-sm">
                <Icon icon={CircleCheck} class="mt-0.5 size-4 shrink-0 text-ok" />
                {ipv6Result(job()) === "sixrd" ? w.worksSixrd : w.works}
              </p>
            </Match>
          </Switch>
        )}
      </Show>
    </div>
  );
}
