import { Radio, TriangleAlert } from "lucide";
import { createEffect, createMemo, createSignal, Show, useContext } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import { api, errorText } from "../../../lib/api";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { JobSteps } from "../../../ui/JobSteps";
import { showToast } from "../../../ui/Toast";

const w = strings.device.wpad;

/**
 * The device's Wi-Fi daemon (wifi.md, Roaming; D-085, D-089): the full wpad, with 802.11v, or the
 * basic one, which admins swap for the full one on a managed device, confirmed first and followed
 * live on the `jobs` topic. Shown on devices with a hostapd.
 */
export function WpadCard(props: { node: Node; facts: NodeFacts | null; admin: boolean }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [asking, setAsking] = createSignal(false);
  const [sending, setSending] = createSignal(false);
  const [started, setStarted] = createSignal<JobId>();
  const wifi = () => props.facts?.capabilities.wifi ?? [];
  const full = () => wifi().includes("11v");
  const managed = () => props.node.mode === "managed";

  /** The job started here, else one running on this node from elsewhere. */
  const job = createMemo(() => {
    const own = started();
    if (own !== undefined && jobs.byId[own]) return jobs.byId[own];
    return Object.values(jobs.byId).find(
      (job) => job.kind === "wpad" && job.node === props.node.id && job.state === "running",
    );
  });
  const running = () => sending() || job()?.state === "running";

  createEffect(
    () => {
      const own = started();
      return own === undefined ? undefined : jobs.byId[own]?.state;
    },
    (state) => {
      const own = jobs.byId[started() ?? -1];
      if (!own || state === undefined || state === "running") return;
      if (state === "succeeded") {
        showToast({ title: w.done, description: w.doneHint, tone: "ok" });
      } else {
        showToast({ title: w.failed, description: own.error?.message, tone: "crit" });
      }
    },
  );

  const install = async () => {
    setSending(true);
    try {
      const job = await api.nodes.wpad(props.node.id);
      addJobs([job]);
      setStarted(job.id);
      setAsking(false);
    } catch (err) {
      showToast({ title: w.failed, description: errorText(err), tone: "crit" });
    } finally {
      setSending(false);
    }
  };

  const note = () => {
    if (full()) return undefined;
    if (!props.admin) return w.adminOnly;
    return managed() ? undefined : w.observe;
  };

  return (
    <Show when={wifi().length > 0}>
      <Card>
        <CardHeader
          title={w.title}
          actions={
            <Show when={props.admin && !full()}>
              <Button
                variant="secondary"
                size="sm"
                disabled={!managed() || running()}
                onClick={() => setAsking(true)}
              >
                {w.install}
              </Button>
            </Show>
          }
        />
        <div class="flex flex-col gap-3 px-4 pb-4">
          <div class="flex items-start gap-3">
            <Icon
              icon={full() ? Radio : TriangleAlert}
              class={["mt-0.5 size-5 shrink-0", full() ? "text-ok" : "text-warn"]}
            />
            <div class="min-w-0 text-sm">
              <p class="font-medium">{full() ? w.full : w.basic}</p>
              <p class="mt-0.5 text-fg-muted">{full() ? w.fullHint : w.basicHint}</p>
              <Show when={note()}>{(text) => <p class="mt-1.5 text-fg-subtle">{text()}</p>}</Show>
            </div>
          </div>
          <Show when={job()?.state === "running" || (job()?.state === "failed" && started())}>
            <div class="rounded-control border border-edge p-2" role="status">
              <JobSteps job={job() as Job} />
            </div>
          </Show>
        </div>
        <Dialog
          open={asking()}
          onOpenChange={setAsking}
          title={w.confirm.title(props.node.name)}
          description={w.confirm.body}
        >
          <p class="px-5 text-fg-muted text-sm">{w.confirm.after}</p>
          <div class="flex justify-end gap-2 px-5 pt-4 pb-5">
            <Button variant="ghost" onClick={() => setAsking(false)}>
              {w.cancel}
            </Button>
            <Button variant="primary" disabled={sending()} onClick={install}>
              {w.confirm.action}
            </Button>
          </div>
        </Dialog>
      </Card>
    </Show>
  );
}
