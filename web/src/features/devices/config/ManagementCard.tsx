import { Eye, ShieldCheck } from "lucide";
import { createEffect, createMemo, createSignal, Show, useContext } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import { api, errorText } from "../../../lib/api";
import { holding } from "../../../lib/config";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { showToast } from "../../../ui/Toast";
import { JobSteps } from "../adopt/JobSteps";

const m = strings.device.management;

const isManageJob = (job: Job) => job.kind === "manage" || job.kind === "unmanage";

/**
 * Management on and off (directive 8; lifecycle.md, Ownership): the node's mode, the revision it
 * holds, and for admins the switch, confirmed first and followed live on the `jobs` topic.
 */
export function ManagementCard(props: { node: Node; admin: boolean }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [asking, setAsking] = createSignal(false);
  const [sending, setSending] = createSignal(false);
  const [started, setStarted] = createSignal<JobId>();
  const managed = () => props.node.mode === "managed";

  /** The job started here, else one running on this node from elsewhere. */
  const job = createMemo(() => {
    const own = started();
    if (own !== undefined && jobs.byId[own]) return jobs.byId[own];
    return Object.values(jobs.byId).find(
      (job) => isManageJob(job) && job.node === props.node.id && job.state === "running",
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
        showToast({ title: own.kind === "manage" ? m.done.on : m.done.off, tone: "ok" });
      } else {
        showToast({ title: m.failed, description: own.error?.message, tone: "crit" });
      }
    },
  );

  const toggle = async () => {
    setSending(true);
    try {
      const started = await api.nodes.manage(props.node.id, !managed());
      addJobs([started]);
      setStarted(started.id);
      setAsking(false);
    } catch (err) {
      showToast({ title: m.failed, description: errorText(err), tone: "crit" });
    } finally {
      setSending(false);
    }
  };

  const held = () => {
    const state = holding(live.config, props.node.id, props.node.mode);
    switch (state.kind) {
      case "observed":
        return m.held.observed;
      case "never":
        return m.held.never;
      case "latest":
        return m.held.latest(state.revision);
      case "behind":
        return m.held.behind(state.revision, state.head);
    }
  };

  const confirm = () => (managed() ? m.confirmOff : m.confirmOn);

  return (
    <Card>
      <CardHeader
        title={m.title}
        actions={
          <Show when={props.admin}>
            <Button
              variant={managed() ? "secondary" : "primary"}
              size="sm"
              disabled={running()}
              onClick={() => setAsking(true)}
            >
              {managed() ? m.turnOff : m.turnOn}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <div class="flex items-start gap-3">
          <Icon
            icon={managed() ? ShieldCheck : Eye}
            class={["mt-0.5 size-5 shrink-0", managed() ? "text-ok" : "text-info"]}
          />
          <div class="min-w-0 text-sm">
            <p class="font-medium">{strings.devices.mode[props.node.mode]}</p>
            <p class="mt-0.5 text-fg-muted">{managed() ? m.managed : m.observe}</p>
            <Show when={live.config.ready}>
              <p class="mt-1.5 text-fg-subtle">{held()}</p>
            </Show>
            <Show when={!props.admin}>
              <p class="mt-1.5 text-fg-subtle">{m.adminOnly}</p>
            </Show>
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
        title={confirm().title(props.node.name)}
        description={confirm().body}
      >
        <div class="flex justify-end gap-2 px-5 pt-2 pb-5">
          <Button variant="ghost" onClick={() => setAsking(false)}>
            {m.cancel}
          </Button>
          <Button variant="primary" disabled={sending()} onClick={toggle}>
            {confirm().action}
          </Button>
        </div>
      </Dialog>
    </Card>
  );
}
