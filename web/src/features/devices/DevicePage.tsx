import { useParams } from "@solidjs/router";
import { useQuery } from "@tanstack/solid-query";
import { ArrowLeft, CircleAlert, Router } from "lucide";
import { createEffect, createMemo, For, Match, Show, Switch, useContext } from "solid-js";
import type { LeaseFacts } from "../../api/gen/LeaseFacts";
import type { NodeDetail } from "../../api/gen/NodeDetail";
import type { NodeFacts } from "../../api/gen/NodeFacts";
import { api } from "../../lib/api";
import { formatBytes, formatCelsius, formatDuration, formatPercent, NONE } from "../../lib/format";
import { createNodeView, type Live, LiveContext, type NodeViewState } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { Sparkline } from "../../ui/Sparkline";
import { Stat } from "../../ui/Stat";
import { ClientsCard, clientRows } from "./ClientsCard";
import { RadiosCard } from "./RadiosCard";
import { describeReason, ModeBadge, StatusBadge } from "./status";

const d = strings.device;

/** `/devices/:id`: one node's live overview. */
export function DevicePage() {
  const params = useParams<{ id: string }>();
  const id = () => Number(params.id);
  return (
    <div class="mx-auto max-w-7xl">
      <a
        href="/devices"
        class="mb-4 inline-flex items-center gap-1.5 rounded-control text-fg-muted text-sm hover:text-fg"
      >
        <Icon icon={ArrowLeft} class="size-4" />
        {d.back}
      </a>
      <Show when={Number.isSafeInteger(id()) && id() > 0} fallback={<NotFound />}>
        <LiveDevice id={id()} />
      </Show>
    </div>
  );
}

function NotFound() {
  return <EmptyState icon={Router} title={d.notFound.title} description={d.notFound.description} />;
}

function LiveDevice(props: { id: number }) {
  const live = useContext(LiveContext);
  const view = createNodeView(live, () => props.id);
  const leases = createGatewayLeases(live, () => props.id);
  createEffect(
    () => view.detail?.node.name ?? strings.devices.title,
    (name) => {
      document.title = `${name} · ${strings.appName}`;
    },
  );
  return (
    <Switch>
      <Match when={!view.ready}>
        <p role="status" class="py-20 text-center text-fg-subtle text-sm">
          {d.loading}…
        </p>
      </Match>
      <Match when={view.detail}>
        {(detail) => <Overview detail={detail()} view={view} leases={leases()} />}
      </Match>
      <Match when={view.missing}>
        <NotFound />
      </Match>
    </Switch>
  );
}

/**
 * The gateway's DHCP leases, which name the clients of access points: fetched once a minute, so
 * looking at an access point doesn't hold the gateway's focus. Empty on the gateway itself.
 */
function createGatewayLeases(live: Live, id: () => number): () => readonly LeaseFacts[] {
  const gateway = createMemo(
    () => live.fleet.nodes.find((node) => node.role === "gateway" && node.id !== id())?.id,
  );
  const query = useQuery(() => ({
    queryKey: ["node", gateway()],
    queryFn: ({ signal }) => api.nodes.get(gateway() ?? 0, signal),
    enabled: gateway() !== undefined,
    refetchInterval: 60_000,
  }));
  return () => (query.isSuccess ? (query.data?.facts?.leases ?? []) : []);
}

function Overview(props: {
  detail: NodeDetail;
  view: NodeViewState;
  leases: readonly LeaseFacts[];
}) {
  const node = () => props.detail.node;
  const status = () => props.detail.status;
  return (
    <div class="flex flex-col gap-4">
      <header class="flex flex-wrap items-start gap-x-4 gap-y-2">
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2.5">
            <h1 class="truncate font-semibold text-2xl tracking-tight">{node().name}</h1>
            <StatusBadge status={status()} />
            <ModeBadge mode={node().mode} />
          </div>
          <p class="mt-1 flex flex-wrap gap-x-2 text-fg-muted text-sm">
            <span>{strings.devices.role[node().role]}</span>
            <Show when={props.detail.facts?.board.model}>
              {(model) => <span>· {model()}</span>}
            </Show>
            <span>
              · <span class="font-mono text-xs">{node().host}</span>
            </span>
          </p>
        </div>
      </header>
      <Show when={status().reasons.length > 0}>
        <Card class="border-warn/40 px-4 py-3" aria-label={strings.devices.state[status().state]}>
          <ul class="flex flex-col gap-1.5 text-sm">
            <For each={status().reasons}>
              {(reason) => (
                <li class="flex items-start gap-2">
                  <Icon icon={CircleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
                  {describeReason(reason)}
                </li>
              )}
            </For>
          </ul>
        </Card>
      </Show>
      <Show
        when={props.detail.facts}
        fallback={<p class="py-12 text-center text-fg-subtle text-sm">{d.waiting}</p>}
      >
        {(facts) => (
          <>
            <Stats facts={facts()} view={props.view} />
            <div class="grid gap-4 lg:grid-cols-3">
              <div class="min-w-0 lg:col-span-2">
                <RadiosCard facts={facts()} />
              </div>
              <Details detail={props.detail} facts={facts()} />
            </div>
            <ClientsCard facts={facts()} leases={props.leases} />
          </>
        )}
      </Show>
    </div>
  );
}

function Stats(props: { facts: NodeFacts; view: NodeViewState }) {
  const s = d.stat;
  const system = () => props.facts.system;
  const memoryPercent = () => {
    const m = system().memory;
    return m.total_bytes > 0 ? (100 * (m.total_bytes - m.available_bytes)) / m.total_bytes : null;
  };
  const meshPeers = () => props.facts.stations.filter((st) => st.peer_link !== null).length;
  const load = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });

  return (
    <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-6">
      <Stat label={s.cpu} value={formatPercent(system().cpu_percent)}>
        <Sparkline values={props.view.cpu} max={100} label={s.cpuTrend} class="text-accent" />
      </Stat>
      <Stat label={s.memory} value={formatPercent(memoryPercent())}>
        <Show
          when={props.view.memory.length > 1}
          fallback={s.memoryOf(formatBytes(system().memory.total_bytes))}
        >
          <Sparkline values={props.view.memory} max={100} label={s.memoryTrend} class="text-info" />
        </Show>
      </Stat>
      <Stat label={s.temperature} value={formatCelsius(system().temperature_c)} />
      <Stat label={s.uptime} value={formatDuration(system().uptime_s)} />
      <Stat label={s.clients} value={clientRows(props.facts).length}>
        <Show when={meshPeers() > 0}>{s.meshPeers(meshPeers())}</Show>
      </Stat>
      <Stat label={s.load} value={load.format(system().load[0])}>
        {system()
          .load.map((l) => load.format(l))
          .join(" · ")}
      </Stat>
    </div>
  );
}

function Details(props: { detail: NodeDetail; facts: NodeFacts }) {
  const t = d.details;
  const board = () => props.facts.board;
  const rows = (): [string, string][] => [
    [t.model, board().model],
    [t.board, board().board_name],
    [t.firmware, `${board().release.distribution} ${board().release.version}`],
    [t.target, board().release.target],
    [t.kernel, board().kernel],
    [t.cpu, board().cpu],
    [t.mac, props.detail.node.mac],
    [t.address, `${props.detail.node.host}:${props.detail.node.port}`],
    [t.mode, strings.devices.mode[props.detail.node.mode]],
  ];
  return (
    <Card class="min-w-0">
      <CardHeader title={t.title} />
      <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 px-4 pb-4 text-sm">
        <For each={rows()}>
          {([label, value]) => (
            <>
              <dt class="text-fg-subtle">{label}</dt>
              <dd class="truncate text-right font-medium" title={value}>
                {value || NONE}
              </dd>
            </>
          )}
        </For>
      </dl>
    </Card>
  );
}
