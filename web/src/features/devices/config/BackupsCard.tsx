import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { Archive, ArchiveRestore, Download } from "lucide";
import { createEffect, createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { Backup } from "../../../api/gen/Backup";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import { api, errorText } from "../../../lib/api";
import { formatDateTime } from "../../../lib/format";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";
import { showToast } from "../../../ui/Toast";
import { backupDetail, concerns, endedCount } from "./backups";

const b = strings.device.backups;

/**
 * A device's configuration backups (lifecycle.md, Backups; D-126), newest first. Admins back it
 * up now, download a backup (it holds the device's passwords and keys) and, on a managed device,
 * restore one's `/etc/config` as a transaction. The jobs are followed live on the `jobs` topic,
 * and the list is read again whenever one that touches the device ends.
 */
export function BackupsCard(props: { node: Node; admin: boolean }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const seen = new Set<number>();
  const ended = createMemo(() => endedCount(Object.values(jobs.byId), props.node.id, seen));
  const backups = useQuery(() => ({
    queryKey: ["backups", props.node.id, ended()],
    queryFn: ({ signal }) => api.nodes.backups.list(props.node.id, signal),
    placeholderData: keepPreviousData,
  }));
  const [sending, setSending] = createSignal(false);
  const [started, setStarted] = createSignal<JobId>();
  const [restoring, setRestoring] = createSignal<Backup>();
  const [error, setError] = createSignal<string>();
  const managed = () => props.node.mode === "managed";

  /** The job started here, else a running one that touches this device. */
  const job = createMemo(() => {
    const own = started();
    if (own !== undefined && jobs.byId[own]) return jobs.byId[own];
    return Object.values(jobs.byId).find(
      (job) => job.state === "running" && concerns(job, props.node.id),
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
      const name = props.node.name;
      const backup = own.kind === "backup";
      if (state === "succeeded") {
        showToast({ title: backup ? b.backedUp(name) : b.restored(name), tone: "ok" });
      } else {
        showToast({
          title: backup ? b.backUpFailed(name) : b.restoreFailed(name),
          description: own.error?.message,
          tone: "crit",
        });
      }
    },
  );

  const start = async (request: () => Promise<Job>) => {
    setSending(true);
    setError(undefined);
    try {
      const job = await request();
      addJobs([job]);
      setStarted(job.id);
      setRestoring(undefined);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSending(false);
    }
  };
  const backUp = () => start(() => api.nodes.backups.create(props.node.id));
  const restore = (backup: Backup) =>
    start(() => api.nodes.backups.restore(props.node.id, backup.id));

  return (
    <Card aria-label={b.title}>
      <CardHeader
        title={b.title}
        description={b.hint}
        actions={
          <Show when={props.admin}>
            <Button
              size="sm"
              variant="secondary"
              icon={Archive}
              disabled={running()}
              onClick={backUp}
            >
              {b.backUp}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <RequestError text={error() ?? (backups.isError ? errorText(backups.error) : undefined)} />
        <Show when={job()?.state === "running" || (job()?.state === "failed" && started())}>
          <div class="rounded-control border border-edge p-2" role="status">
            <JobSteps job={job() as Job} />
          </div>
        </Show>
        <Show when={backups.isSuccess && backups.data.length === 0}>
          <p class="text-fg-subtle text-sm">{b.none}</p>
        </Show>
        <Show when={backups.isSuccess && backups.data.length > 0}>
          <ul class="flex flex-col divide-y divide-edge rounded-control border border-edge">
            <For each={backups.data ?? []}>
              {(backup) => {
                const when = () => formatDateTime(backup.taken_at);
                return (
                  <li class="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2">
                    <div class="min-w-0 flex-1 text-sm">
                      <p class="font-medium tabular-nums">{b.taken(when())}</p>
                      <p class="text-fg-muted text-xs">{backupDetail(backup)}</p>
                    </div>
                    <Show when={props.admin}>
                      <div class="flex gap-1.5">
                        <a
                          href={api.nodes.backups.archiveUrl(props.node.id, backup.id)}
                          download
                          aria-label={b.downloadLabel(when())}
                          class="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-control border border-edge bg-surface px-2.5 font-medium text-fg text-xs hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
                        >
                          <Icon icon={Download} class="size-3.5" />
                          {b.download}
                        </a>
                        <Show when={managed()}>
                          <Button
                            size="sm"
                            variant="ghost"
                            icon={ArchiveRestore}
                            aria-label={b.restoreLabel(when())}
                            disabled={running()}
                            onClick={() => setRestoring(backup)}
                          >
                            {b.restore}
                          </Button>
                        </Show>
                      </div>
                    </Show>
                  </li>
                );
              }}
            </For>
          </ul>
        </Show>
        <Show when={props.admin && !managed()}>
          <p class="text-fg-subtle text-xs">{b.observe}</p>
        </Show>
        <Show when={!props.admin}>
          <p class="text-fg-subtle text-xs">{b.adminOnly}</p>
        </Show>
      </div>
      <Dialog
        open={restoring() !== undefined}
        onOpenChange={(open) => {
          if (!open) setRestoring(undefined);
        }}
        title={b.confirm.title(props.node.name)}
        description={b.confirm.body(formatDateTime(restoring()?.taken_at ?? 0))}
      >
        <div class="flex flex-col gap-2 px-5 text-fg-muted text-sm">
          <p>{b.confirm.rest}</p>
          <p>{b.confirm.drift}</p>
        </div>
        <DialogFooter class="px-5 pt-4 pb-5">
          <Button variant="ghost" onClick={() => setRestoring(undefined)}>
            {b.cancel}
          </Button>
          <Button
            variant="primary"
            disabled={sending()}
            onClick={() => {
              const backup = restoring();
              if (backup) void restore(backup);
            }}
          >
            {b.confirm.action}
          </Button>
        </DialogFooter>
      </Dialog>
    </Card>
  );
}
