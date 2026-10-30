import { CircleCheck, Fingerprint } from "lucide";
import { createSignal, For, Match, Show, Switch, untrack } from "solid-js";
import type { BatchAdoptBody } from "../../../api/gen/BatchAdoptBody";
import type { Discovered } from "../../../api/gen/Discovered";
import type { Job } from "../../../api/gen/Job";
import type { Refused } from "../../../api/gen/Refused";
import { ApiError, api } from "../../../lib/api";
import { type JobsState, sshTarget } from "../../../lib/jobs";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { TextField } from "../../../ui/TextField";
import { JobErrorNote, RequestError } from "./JobOutcome";
import { currentStep, jobProgress } from "./JobSteps";

const s = strings.adopt;

export interface BatchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The discovered devices to adopt, each with an address and the host key its probe read. */
  nodes: readonly Discovered[];
  jobs: JobsState;
  addJobs: (jobs: readonly Job[]) => void;
}

/** Adopts the selected discovered devices with one password: fingerprints, then a job each. */
export function BatchDialog(props: BatchDialogProps) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={s.batch.title(props.nodes.length)}
      description={s.batch.description}
      initialFocus={() =>
        document.querySelector<HTMLElement>("[data-autofocus] input") ?? undefined
      }
      class="max-w-xl"
    >
      <Batch {...props} />
    </Dialog>
  );
}

/** A device's label: its hostname, else its address. */
export function discoveredName(node: Discovered): string {
  return node.hostname ?? node.ip ?? node.mac;
}

function Batch(props: BatchDialogProps) {
  // The devices as they were when the dialog opened: discovery drops them once adopted.
  const nodes = untrack(() => [...props.nodes]);
  const [password, setPassword] = createSignal("");
  const [remember, setRemember] = createSignal(false);
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [started, setStarted] = createSignal<{ jobs: Job["id"][]; refused: Refused[] }>();

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (pending()) return;
    setPending(true);
    setError(undefined);
    const body: BatchAdoptBody = {
      nodes: nodes.flatMap((node) =>
        node.ip && node.host_key ? [{ host: node.ip, host_key: node.host_key.host_key }] : [],
      ),
    };
    if (password()) {
      body.password = password();
      if (remember()) body.remember_password = true;
    }
    try {
      const result = await api.adopt.batch(body);
      props.addJobs(result.jobs);
      setStarted({ jobs: result.jobs.map((job) => job.id), refused: result.refused });
    } catch (e) {
      setError(e instanceof ApiError ? e.message : strings.auth.unreachable.title);
    } finally {
      setPending(false);
    }
  };

  const byTarget = new Map(nodes.map((node) => [sshTarget(node.ip ?? ""), node]));
  const label = (target: string) => {
    const node = byTarget.get(target);
    return node ? discoveredName(node) : target;
  };
  const jobs = () =>
    (started()?.jobs ?? []).flatMap((id) => {
      const job = props.jobs.byId[id];
      return job ? [job] : [];
    });
  const succeeded = () => jobs().filter((job) => job.state === "succeeded").length;

  return (
    <div class="px-5 pt-1 pb-5">
      <Show
        when={started()}
        fallback={
          <form class="flex flex-col gap-4" onSubmit={submit}>
            <RequestError text={error()} />
            <ul class="flex max-h-64 flex-col divide-y divide-edge overflow-y-auto rounded-control border border-edge">
              <For each={nodes}>
                {(node) => (
                  <li class="px-3 py-2">
                    <p class="flex items-baseline justify-between gap-3 text-sm">
                      <span class="truncate font-medium">{discoveredName(node)}</span>
                      <span class="shrink-0 font-mono text-fg-muted text-xs">{node.ip}</span>
                    </p>
                    <p class="mt-1 flex items-center gap-1.5 text-fg-muted text-xs">
                      <Icon icon={Fingerprint} class="size-3.5 shrink-0 text-accent" />
                      <span class="break-all font-mono text-fg">{node.host_key?.fingerprint}</span>
                    </p>
                  </li>
                )}
              </For>
            </ul>
            <div data-autofocus>
              <TextField
                label={s.password}
                name="password"
                type="password"
                autocomplete="off"
                value={password()}
                onInput={(value) => {
                  setPassword(value);
                  if (!value) setRemember(false);
                }}
                hint={s.batch.passwordHint}
              />
            </div>
            <Checkbox
              label={s.remember}
              hint={s.rememberHint}
              checked={remember()}
              onChange={setRemember}
              disabled={!password()}
            />
            <div class="mt-2 flex flex-wrap justify-end gap-2">
              <Button onClick={() => props.onOpenChange(false)}>{s.cancel}</Button>
              <Button type="submit" variant="primary" disabled={pending()}>
                {pending() ? s.confirm.starting : s.batch.submit(nodes.length)}
              </Button>
            </div>
          </form>
        }
      >
        {(result) => (
          <div class="flex flex-col gap-3">
            <ul class="flex flex-col gap-2">
              <For each={jobs()}>{(job) => <JobRow job={job} label={label(job.target)} />}</For>
            </ul>
            <Show when={result().refused.length > 0}>
              <div>
                <h3 class="font-medium text-fg-muted text-xs">{s.batch.refused}</h3>
                <ul class="mt-1 flex flex-col gap-1 text-sm">
                  <For each={result().refused}>
                    {(refused) => (
                      <li>
                        <span class="font-medium">
                          {label(sshTarget(refused.host, refused.port))}
                        </span>
                        <span class="text-fg-muted">: {refused.reason}</span>
                      </li>
                    )}
                  </For>
                </ul>
              </div>
            </Show>
            <div class="mt-2 flex flex-wrap items-center justify-end gap-3">
              <p role="status" class="mr-auto text-fg-muted text-sm">
                {s.batch.done(succeeded(), result().jobs.length)}
              </p>
              <Button onClick={() => props.onOpenChange(false)}>{s.progress.close}</Button>
            </div>
          </div>
        )}
      </Show>
    </div>
  );
}

/** One device's adoption: where it is, as a bar, and why it failed. */
function JobRow(props: { job: Job; label: string }) {
  const step = () => currentStep(props.job);
  return (
    <li class="rounded-control border border-edge px-3 py-2.5">
      <div class="flex items-baseline justify-between gap-3 text-sm">
        <span class="truncate font-medium">{props.label}</span>
        <Switch>
          <Match when={props.job.state === "succeeded"}>
            <span class="flex shrink-0 items-center gap-1 text-fg text-xs">
              <Icon icon={CircleCheck} class="size-3.5 text-ok" />
              {s.stepState.done}
            </span>
          </Match>
          <Match when={step()}>
            {(step) => (
              <span class="shrink-0 text-fg-muted text-xs">
                {s.step[step().name]} · {s.stepState[step().state]}
              </span>
            )}
          </Match>
        </Switch>
      </div>
      <div
        role="progressbar"
        aria-label={props.label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(jobProgress(props.job) * 100)}
        class="mt-2 h-1 overflow-hidden rounded-full bg-raised"
      >
        <div
          class={[
            "h-full rounded-full transition-[width] duration-500",
            props.job.state === "failed"
              ? "bg-crit"
              : props.job.state === "succeeded"
                ? "bg-ok"
                : "bg-accent",
          ]}
          style={{ width: `${jobProgress(props.job) * 100}%` }}
        />
      </div>
      <Show when={props.job.error}>
        {(error) => (
          <div class="mt-2">
            <JobErrorNote error={error()} compact />
          </div>
        )}
      </Show>
    </li>
  );
}
