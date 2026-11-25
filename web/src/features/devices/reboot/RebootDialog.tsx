import { RotateCw } from "lucide";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { NodeSummary } from "../../../api/gen/NodeSummary";
import { api, errorText } from "../../../lib/api";
import type { JobsState } from "../../../lib/jobs";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { JobNodes } from "../../../ui/JobNodes";
import { JobErrorNote, JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { showToast } from "../../../ui/Toast";
import { canReboot, defaultPick, rebootOrder, runningReboot } from "./reboot";

const r = strings.devices.reboot;

/**
 * Restarting devices (network.md, Restarts; D-137): one device is asked about, several are picked
 * from (the managed ones that answer, at first). Once started, the job is followed live on the
 * `jobs` topic, as is a restart of these devices started elsewhere.
 */
export function RebootDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The devices offered. */
  nodes: readonly NodeSummary[];
  jobs: JobsState;
  addJobs: (jobs: readonly Job[]) => void;
}) {
  const [picked, setPicked] = createSignal<number[]>([]);
  const [started, setStarted] = createSignal<JobId>();
  const [sending, setSending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const single = () => (props.nodes.length === 1 ? props.nodes[0] : undefined);
  const job = createMemo(() => {
    const own = started();
    if (own !== undefined && props.jobs.byId[own]) return props.jobs.byId[own];
    return runningReboot(
      props.jobs,
      props.nodes.map((node) => node.id),
    );
  });
  const pickedNodes = () => props.nodes.filter((node) => picked().includes(node.id));

  createEffect(
    () => props.open,
    (open) => {
      if (!open) return;
      const one = single();
      setPicked(one ? (canReboot(one) ? [one.id] : []) : defaultPick(props.nodes));
      setError(undefined);
      const own = started();
      if (own !== undefined && props.jobs.byId[own]?.state !== "running") setStarted(undefined);
    },
  );

  createEffect(
    () => {
      const own = started();
      return own === undefined ? undefined : props.jobs.byId[own]?.state;
    },
    (state) => {
      const own = props.jobs.byId[started() ?? -1];
      if (!own || state === undefined || state === "running") return;
      if (state === "succeeded") showToast({ title: r.done(own.nodes.length), tone: "ok" });
      else showToast({ title: r.failed, description: own.error?.message, tone: "crit" });
    },
  );

  const start = async () => {
    setSending(true);
    setError(undefined);
    try {
      const job = await api.reboots.start({ nodes: picked() });
      props.addJobs([job]);
      setStarted(job.id);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSending(false);
    }
  };

  const toggle = (id: number, on: boolean) =>
    setPicked((ids) => (on ? [...ids, id] : ids.filter((other) => other !== id)));

  const hint = (node: NodeSummary) => {
    if (!canReboot(node)) return r.observe;
    if (node.status.state === "offline") return r.offline;
    return node.role === "gateway" ? r.gatewayLast : undefined;
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={single() ? r.titleOne(single()?.name ?? "") : r.title}
      description={single() ? undefined : r.description}
    >
      <div class="flex flex-col gap-4 px-5 pt-2 pb-5">
        <Show
          when={job()}
          fallback={
            <>
              <Show when={!single()}>
                <fieldset class="flex flex-col gap-2.5">
                  <legend class="mb-2 font-medium text-sm">{r.pick}</legend>
                  <For each={rebootOrder(props.nodes)}>
                    {(node) => (
                      <Checkbox
                        label={node.name}
                        hint={hint(node)}
                        checked={picked().includes(node.id)}
                        disabled={!canReboot(node)}
                        onChange={(on) => toggle(node.id, on)}
                      />
                    )}
                  </For>
                </fieldset>
              </Show>
              <div
                role="alert"
                class="flex flex-col gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm"
              >
                <p>{r.confirm}</p>
                <Show when={pickedNodes().some((node) => node.role === "gateway")}>
                  <p>{r.confirmGateway}</p>
                </Show>
                <Show when={picked().length > 1}>
                  <p class="text-fg-muted">{r.stops}</p>
                </Show>
              </div>
              <RequestError text={error()} />
              <DialogFooter>
                <Button type="button" variant="ghost" onClick={() => props.onOpenChange(false)}>
                  {r.cancel}
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  icon={RotateCw}
                  disabled={picked().length === 0 || sending()}
                  onClick={() => void start()}
                >
                  {sending() ? r.starting : r.start(picked().length)}
                </Button>
              </DialogFooter>
            </>
          }
        >
          {(job) => (
            <>
              <JobNodes
                nodes={job().nodes}
                label={r.devices}
                states={r.state}
                running={job().state === "running"}
                doneState="verified"
              />
              <Show when={job().state === "running"}>
                <p role="status" class="text-fg-muted text-xs">
                  {r.running}
                </p>
              </Show>
              <Show when={job().state === "succeeded"}>
                <JobSuccessNote text={r.done(job().nodes.length)} />
              </Show>
              <Show when={job().error}>{(error) => <JobErrorNote error={error()} />}</Show>
              <DialogFooter>
                <Button type="button" onClick={() => props.onOpenChange(false)}>
                  {r.close}
                </Button>
              </DialogFooter>
            </>
          )}
        </Show>
      </div>
    </Dialog>
  );
}
