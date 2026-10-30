import { useNavigate, useSearchParams } from "@solidjs/router";
import { Plus, RadioTower, Router } from "lucide";
import { createEffect, createSignal, Show, useContext } from "solid-js";
import type { Discovered } from "../../api/gen/Discovered";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { formatDuration, formatPercent, NONE } from "../../lib/format";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { type Column, Table } from "../../ui/Table";
import { AdoptDialog } from "./adopt/AdoptDialog";
import { BatchDialog } from "./adopt/BatchDialog";
import { DiscoveredCard } from "./adopt/DiscoveredCard";
import { ModeBadge, StatusBadge } from "./status";

const c = strings.devices.columns;

const columns: Column<NodeSummary>[] = [
  {
    id: "name",
    header: c.name,
    cell: (node) => (
      <span class="flex min-w-0 items-center gap-2.5" title={strings.devices.role[node.role]}>
        <Icon
          icon={node.role === "gateway" ? Router : RadioTower}
          class="size-4 shrink-0 text-fg-subtle"
        />
        <span class="truncate font-medium">{node.name}</span>
        <span class="sr-only">{strings.devices.role[node.role]}</span>
      </span>
    ),
    class: "min-w-40",
  },
  {
    id: "status",
    header: c.status,
    cell: (node) => (
      <span class="flex items-center gap-1.5">
        <StatusBadge status={node.status} />
        <ModeBadge mode={node.mode} />
      </span>
    ),
  },
  { id: "model", header: c.model, cell: (node) => node.model ?? NONE, class: "whitespace-nowrap" },
  {
    id: "address",
    header: c.address,
    cell: (node) => <span class="font-mono text-xs">{node.host}</span>,
  },
  { id: "firmware", header: c.firmware, cell: (node) => node.version ?? NONE },
  { id: "clients", header: c.clients, cell: (node) => node.clients ?? NONE, align: "end" },
  { id: "cpu", header: c.cpu, cell: (node) => formatPercent(node.cpu_percent), align: "end" },
  {
    id: "memory",
    header: c.memory,
    cell: (node) => formatPercent(node.memory_percent),
    align: "end",
  },
  {
    id: "uptime",
    header: c.uptime,
    cell: (node) => formatDuration(node.uptime_s),
    align: "end",
    class: "whitespace-nowrap",
  },
];

/** The wizard's place in the URL (`?adopt=1&host=…`), so ⌘K and links can open it. */
type AdoptParams = { adopt?: string; host?: string };

/**
 * Every adopted node with its live status and headline numbers; a row opens the node. Below them,
 * the discovered devices, and the adoption wizard for one device or several.
 */
export function DevicesPage() {
  const live = useContext(LiveContext);
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams<AdoptParams>();
  const [jobs, addJobs] = createJobsView(live);
  const [batch, setBatch] = createSignal<Discovered[]>();
  const openAdopt = (host?: string) => setParams({ adopt: "1", host }, { replace: true });
  // Only while open: Kobalte also reports a close as the page unmounts, when navigating away.
  const closeAdopt = () => {
    if (params.adopt) setParams({ adopt: undefined, host: undefined }, { replace: true });
  };
  const adoptButton = () => (
    <Button size="sm" variant="primary" icon={Plus} onClick={() => openAdopt()}>
      {strings.adopt.open}
    </Button>
  );
  createEffect(
    () => strings.devices.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <>
      <Show
        when={live.fleet.ready}
        fallback={
          <p role="status" class="py-20 text-center text-fg-subtle text-sm">
            {strings.devices.loading}…
          </p>
        }
      >
        <Show
          when={live.fleet.nodes.length > 0}
          fallback={
            <EmptyState
              icon={Router}
              title={strings.devices.empty.title}
              description={strings.devices.empty.description}
            >
              <div class="mt-6">{adoptButton()}</div>
            </EmptyState>
          }
        >
          <Card class="mx-auto max-w-7xl overflow-hidden">
            <CardHeader
              title={strings.devices.title}
              description={strings.devices.count(live.fleet.nodes.length)}
              actions={adoptButton()}
            />
            <Table
              label={strings.devices.title}
              columns={columns}
              rows={live.fleet.nodes}
              rowKey={(node) => String(node.id)}
              onRowClick={(node) => navigate(`/devices/${node.id}`)}
            />
          </Card>
          {/* Below the list, so that it appearing never moves the rows. */}
          <DiscoveredCard jobs={jobs} onAdopt={openAdopt} onAdoptMany={setBatch} />
        </Show>
      </Show>
      <AdoptDialog
        open={params.adopt === "1"}
        onOpenChange={(open) => {
          if (!open) closeAdopt();
        }}
        host={params.host}
        jobs={jobs}
        addJobs={addJobs}
        onOpenDevice={(id) => navigate(`/devices/${id}`)}
      />
      <BatchDialog
        open={batch() !== undefined}
        onOpenChange={(open) => {
          if (!open) setBatch(undefined);
        }}
        nodes={batch() ?? []}
        jobs={jobs}
        addJobs={addJobs}
      />
    </>
  );
}
