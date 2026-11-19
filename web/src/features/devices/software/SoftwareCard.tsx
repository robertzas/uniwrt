import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { Lock, PackagePlus } from "lucide";
import { createEffect, createMemo, createSignal, Show, untrack, useContext } from "solid-js";
import type { InstalledPackage } from "../../../api/gen/InstalledPackage";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { PackageChanges } from "../../../api/gen/PackageChanges";
import { api, errorText } from "../../../lib/api";
import { formatDateTime } from "../../../lib/format";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { JobSteps } from "../../../ui/JobSteps";
import { Segmented } from "../../../ui/Segmented";
import { type Column, Table } from "../../../ui/Table";
import { TextField } from "../../../ui/TextField";
import { showToast } from "../../../ui/Toast";
import { Tooltip } from "../../../ui/Tooltip";
import { CatalogDialog, createCatalogReader } from "./CatalogDialog";
import { ChangesDialog } from "./ChangesDialog";
import {
  type ChangeKind,
  changeCount,
  concerns,
  endedCount,
  NO_CHANGES,
  pruned,
  removeBlock,
  type Shown,
  shownPackages,
  toggled,
} from "./software";

const t = strings.device.software;

/**
 * A device's installed packages (network.md, Package manager; D-139), searched, on purpose or as
 * dependencies, each lock with its reason. On a managed device admins pick removals here and
 * installs and upgrades from its feeds, confirm the list, and follow the `packages` job live; the
 * list is read again whenever one ends. Read-only in observe mode.
 */
export function SoftwareCard(props: { node: Node; facts: NodeFacts | null; admin: boolean }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const seen = new Set<number>();
  const ended = createMemo(() => endedCount(Object.values(jobs.byId), props.node.id, seen));
  const listing = useQuery(() => ({
    queryKey: ["node-packages", props.node.id, ended()],
    queryFn: ({ signal }) => api.nodes.packages(props.node.id, signal),
    placeholderData: keepPreviousData,
  }));
  const reader = createCatalogReader(() => props.node.id);
  const [shown, setShown] = createSignal<Shown>("explicit");
  const [query, setQuery] = createSignal("");
  const [changes, setChanges] = createSignal<PackageChanges>(NO_CHANGES);
  const [browsing, setBrowsing] = createSignal(false);
  const [reviewing, setReviewing] = createSignal(false);
  const [started, setStarted] = createSignal<JobId>();
  const installed = (): readonly InstalledPackage[] => listing.data?.packages ?? [];
  const requiredUnknown = () => listing.data?.required_unknown ?? null;
  const canChange = () => props.admin && props.node.mode === "managed";

  /** The job started here, else one running on this node from elsewhere. */
  const job = createMemo(() => {
    const own = started();
    if (own !== undefined && jobs.byId[own]) return jobs.byId[own];
    return Object.values(jobs.byId).find(
      (job) => job.state === "running" && concerns(job, props.node.id),
    );
  });
  const running = () => job()?.state === "running";
  const ownJob = () => {
    const own = started();
    return own === undefined ? undefined : jobs.byId[own];
  };

  // The packages changed: what the feeds offered is stale, and so is what was chosen of it.
  createEffect(ended, () => reader.forget(), { defer: true });
  createEffect(
    () => listing.data?.packages,
    (packages) => {
      if (packages) setChanges((now) => pruned(now, packages));
    },
  );

  createEffect(
    () => ownJob()?.state,
    (state) => {
      const own = untrack(ownJob);
      if (!own || state === undefined || state === "running") return;
      if (state === "succeeded") {
        setChanges(NO_CHANGES);
        showToast({ title: t.done(props.node.name), tone: "ok" });
      } else {
        showToast({
          title: t.failed(props.node.name),
          description: own.error?.message,
          tone: "crit",
        });
      }
    },
  );

  const toggle = (kind: ChangeKind, name: string, on: boolean) =>
    setChanges((now) => toggled(now, kind, name, on));

  const review = () => {
    if (ownJob()?.state !== "running") setStarted(undefined);
    setReviewing(true);
  };

  const count = (which: Shown) => shownPackages(installed(), which, "").length;
  const rows = createMemo(() => shownPackages(installed(), shown(), query()));

  const columns = (): Column<InstalledPackage>[] => [
    { id: "name", header: t.columns.name, cell: (pkg) => <NameCell pkg={pkg} /> },
    {
      id: "version",
      header: t.columns.version,
      cell: (pkg) => <span class="font-mono text-fg-muted text-xs">{pkg.version}</span>,
    },
    {
      id: "kind",
      header: t.columns.kind,
      cell: (pkg) => (
        <span class="text-fg-muted">{pkg.explicit ? t.kind.explicit : t.kind.dependency}</span>
      ),
    },
    ...(canChange()
      ? [
          {
            id: "remove",
            header: t.columns.remove,
            align: "end" as const,
            class: "w-20",
            cell: (pkg: InstalledPackage) => (
              <Checkbox
                hideLabel
                class="justify-end"
                label={t.remove(pkg.name)}
                checked={changes().remove.includes(pkg.name)}
                disabled={running() || removeBlock(pkg, requiredUnknown()) !== undefined}
                onChange={(on) => toggle("remove", pkg.name, on)}
              />
            ),
          },
        ]
      : []),
  ];

  const note = () => {
    if (!props.admin) return t.adminOnly;
    return props.node.mode === "managed" ? undefined : t.observe;
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={canChange()}>
            <Button
              variant="secondary"
              size="sm"
              icon={PackagePlus}
              disabled={running() || !listing.data?.taken_at}
              onClick={() => setBrowsing(true)}
            >
              {t.add}
            </Button>
          </Show>
        }
      />
      <Show
        when={listing.data}
        fallback={
          <p role="status" class="px-4 pb-4 text-fg-subtle text-sm">
            {listing.isError ? errorText(listing.error) : `${t.loading}…`}
          </p>
        }
      >
        {(data) => (
          <div class="flex flex-col gap-3 px-4 pb-4">
            <Show
              when={data().taken_at}
              fallback={<p class="text-fg-subtle text-sm">{t.notRead}</p>}
            >
              {(taken) => (
                <p class="text-fg-muted text-sm">
                  {t.summary(data().packages.length, props.facts?.capabilities.package_manager)}
                  {" · "}
                  {t.readAt(formatDateTime(taken()))}
                </p>
              )}
            </Show>
            <Show when={note()}>{(text) => <p class="text-fg-subtle text-sm">{text()}</p>}</Show>
            <Show when={canChange() && data().required_unknown}>
              {(why) => (
                <p class="rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-sm">
                  {t.requiredUnknown(why())}
                </p>
              )}
            </Show>
            <Show when={data().packages.length > 0}>
              <div class="flex flex-wrap items-end justify-between gap-3">
                <div class="min-w-48 flex-1">
                  <TextField label={t.search} value={query()} onInput={setQuery} />
                </div>
                <Segmented
                  label={t.shown.label}
                  value={shown()}
                  onChange={setShown}
                  options={[
                    { value: "explicit", label: t.shown.explicit(count("explicit")) },
                    { value: "dependency", label: t.shown.dependency(count("dependency")) },
                    { value: "all", label: t.shown.all(count("all")) },
                  ]}
                />
              </div>
              <Table
                label={t.title}
                columns={columns()}
                rows={rows()}
                rowKey={(pkg) => pkg.name}
                empty={t.none}
                class="max-h-[60vh] rounded-control border border-edge"
              />
            </Show>
            <Show when={running()}>
              <div class="rounded-control border border-edge p-2" role="status">
                <p class="px-2 pb-1 text-fg-muted text-xs">{t.running}</p>
                <JobSteps job={job() as Job} />
              </div>
            </Show>
            <Show when={canChange() && changeCount(changes()) > 0}>
              <div class="flex flex-wrap items-center justify-between gap-2 rounded-control border border-accent/40 bg-accent/5 px-3 py-2">
                <p class="text-sm">{t.pending(changes())}</p>
                <div class="flex gap-2">
                  <Button variant="ghost" size="sm" onClick={() => setChanges(NO_CHANGES)}>
                    {t.clear}
                  </Button>
                  <Button variant="primary" size="sm" disabled={running()} onClick={review}>
                    {t.review}
                  </Button>
                </div>
              </div>
            </Show>
          </div>
        )}
      </Show>
      <CatalogDialog
        open={browsing()}
        onOpenChange={setBrowsing}
        name={props.node.name}
        reader={reader}
        installed={installed()}
        changes={changes()}
        onToggle={toggle}
      />
      <ChangesDialog
        open={reviewing()}
        onOpenChange={setReviewing}
        node={props.node}
        changes={changes()}
        catalog={reader.state().catalog}
        installed={installed()}
        job={ownJob()}
        onStarted={(job) => {
          addJobs([job]);
          setStarted(job.id);
        }}
      />
    </Card>
  );
}

/** A package's name with its lock and whether it comes with the firmware, each explained. */
function NameCell(props: { pkg: InstalledPackage }) {
  return (
    <div class="flex items-center gap-2">
      <span class="font-mono text-sm">{props.pkg.name}</span>
      <Show when={props.pkg.lock}>
        {(lock) => (
          <Tooltip content={t.lockWhy[lock()]} class="rounded-full">
            <Badge tone="info" icon={Lock}>
              {t.lock[lock()]}
            </Badge>
          </Tooltip>
        )}
      </Show>
      <Show when={props.pkg.with_firmware}>
        <Tooltip content={t.firmwareWhy} class="rounded-full">
          <Badge>{t.firmware}</Badge>
        </Tooltip>
      </Show>
    </div>
  );
}
