import type { JSX } from "@solidjs/web";
import { useQuery } from "@tanstack/solid-query";
import { ShieldAlert, TriangleAlert, Waypoints } from "lucide";
import { createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { TopoNode } from "../../../api/gen/TopoNode";
import { api, errorText } from "../../../lib/api";
import { formatDateTime, formatPercent, formatTime, NONE } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { EmptyState } from "../../../ui/EmptyState";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { Sparkline } from "../../../ui/Sparkline";
import { createMetricsFeed } from "../../../viz/timeseries/feed";
import { createLiveGraph, type Graph } from "../../../viz/topology/graph";
import { canGoDown, impactOf, scoreClass, weakestFactor } from "../../../viz/topology/health";
import { vertexLabel } from "../../../viz/topology/scene";
import {
  failuresOf,
  lostNodes,
  type MeshLink,
  meshLinksOf,
  type NodeFailure,
  strandedPorts,
  vertexOf,
} from "./mesh";

const t = strings.device.mesh;
const td = strings.topology.details;
const CELL = "py-2 pr-3";
const HEAD = "py-1.5 pr-3 font-medium";

/** What a vertex is called. */
type Labeler = (id: string) => string;

/**
 * A device's Mesh tab (features/mesh.md, Telemetry & health): how it reaches the gateway and
 * what reaches the gateway through it, its links to other devices with their health and
 * history, the ports it holds for the backhaul with nothing on them, and its failed cables.
 */
export function MeshTab(props: { node: Node; facts: NodeFacts | null }) {
  const live = useContext(LiveContext);
  const graph = createLiveGraph(live);
  const names = createMemo(() => new Map(live.fleet.nodes.map((node) => [node.id, node.name])));
  const vertex = () => vertexOf(props.node.id);
  const labelOf: Labeler = (id) => {
    const node = graph()?.nodes.get(id);
    return node
      ? vertexLabel(node, (n) => (n.node === null ? undefined : names().get(n.node)))
      : id;
  };
  const links = createMemo(() => {
    const current = graph();
    return current ? meshLinksOf(current, vertex()) : [];
  });
  const stranded = createMemo(() => strandedPorts(props.facts, links()));
  return (
    <div class="flex flex-col gap-4">
      <For each={stranded()}>
        {(port) => (
          <p
            role="alert"
            class="flex items-start gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-sm"
          >
            <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
            {t.stranded(port)}
          </p>
        )}
      </For>
      <Show
        when={graph()}
        fallback={
          <p role="status" class="py-12 text-center text-fg-subtle text-sm">
            {t.waiting}
          </p>
        }
      >
        {(current) => (
          <Show
            when={current().nodes.get(vertex())}
            fallback={<EmptyState icon={Waypoints} title={t.links.title} description={t.absent} />}
          >
            {(me) => (
              <>
                <PathCard graph={current()} me={me()} labelOf={labelOf} />
                <LinksCard links={links()} labelOf={labelOf} graph={current()} />
              </>
            )}
          </Show>
        )}
      </Show>
      <FailuresCard node={props.node.id} names={names()} />
    </div>
  );
}

/** A vertex by name, linked to its Mesh tab when it is an adopted node. */
function PeerName(props: { graph: Graph; id: string; labelOf: Labeler }) {
  const node = () => props.graph.nodes.get(props.id)?.node ?? null;
  return (
    <Show when={node()} fallback={<span>{props.labelOf(props.id)}</span>}>
      {(id) => (
        <a href={`/devices/${id()}?tab=mesh`} class="rounded-control text-accent hover:underline">
          {props.labelOf(props.id)}
        </a>
      )}
    </Show>
  );
}

function Row(props: { label: string; children: JSX.Element }) {
  return (
    <>
      <dt class="text-fg-subtle">{props.label}</dt>
      <dd class="min-w-0 font-medium">{props.children}</dd>
    </>
  );
}

function PathCard(props: { graph: Graph; me: TopoNode; labelOf: Labeler }) {
  const impact = createMemo(() =>
    canGoDown(props.graph, props.me.id) ? impactOf(props.graph, props.me.id) : undefined,
  );
  const hops = () => {
    const me = props.me;
    if (me.kind === "gateway") return t.path.gateway;
    if (me.gateway_hops === null) return t.path.unreached;
    return td.hopCount(me.gateway_hops, me.wireless_hops);
  };
  const reached = () => props.me.kind !== "gateway" && props.me.gateway_hops !== null;
  return (
    <Card aria-label={t.path.title}>
      <CardHeader title={t.path.title} description={t.path.description} />
      <dl class="grid grid-cols-[minmax(8rem,auto)_minmax(0,1fr)] gap-x-6 gap-y-2 px-4 pb-4 text-sm">
        <Row label={td.gatewayHops}>{hops()}</Row>
        <Show when={reached()}>
          <Row label={t.path.dependsOn}>
            <Show when={props.me.depends_on} fallback={t.path.redundant}>
              {(id) => <PeerName graph={props.graph} id={id()} labelOf={props.labelOf} />}
            </Show>
          </Row>
        </Show>
        <Show when={impact()}>
          {(lost) => (
            <>
              <Row label={t.path.ifDown}>{td.impact(lost().devices, lost().clients)}</Row>
              <Show when={lostNodes(props.graph, lost().lost).length > 0}>
                <Row label={t.path.lost}>
                  <span class="inline-flex flex-wrap gap-x-2">
                    <For each={lostNodes(props.graph, lost().lost)}>
                      {(id) => (
                        <PeerName graph={props.graph} id={vertexOf(id)} labelOf={props.labelOf} />
                      )}
                    </For>
                  </span>
                </Row>
              </Show>
            </>
          )}
        </Show>
      </dl>
    </Card>
  );
}

function LinksCard(props: { links: readonly MeshLink[]; labelOf: Labeler; graph: Graph }) {
  const meshIds = createMemo(() =>
    props.links
      .filter((link) => link.edge.medium === "mesh")
      .map((link) => link.edge.id)
      .sort(),
  );
  const feed = createMetricsFeed(() =>
    meshIds().length === 0
      ? undefined
      : {
          queries: meshIds().map((id) => ({ entity: `link:${id}`, metrics: ["quality"] as const })),
          range: { preset: "24h" },
          width: 120,
        },
  );
  const history = (id: string) =>
    (feed.series(`link:${id}`, "quality")?.avg ?? []).filter((v): v is number => v !== null);
  return (
    <Card aria-label={t.links.title}>
      <CardHeader title={t.links.title} description={t.links.description} />
      <div class="px-4 pb-4">
        <Show
          when={props.links.length > 0}
          fallback={<p class="py-4 text-fg-subtle text-sm">{t.links.empty}</p>}
        >
          <div class="overflow-x-auto">
            <table class="w-full text-sm" aria-label={t.links.label}>
              <thead class="text-left text-fg-muted text-xs">
                <tr>
                  <th class={HEAD}>{t.links.peer}</th>
                  <th class={HEAD}>{t.links.link}</th>
                  <th class={HEAD}>{t.links.health}</th>
                  <th class={HEAD}>{t.links.drops}</th>
                  <th class="w-32 py-1.5 font-medium">{t.links.history}</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-edge">
                <For each={props.links} keyed={(link) => link.edge.id}>
                  {(link) => (
                    <tr class="align-top" aria-label={props.labelOf(link().peer)}>
                      <td class={CELL}>
                        <PeerName graph={props.graph} id={link().peer} labelOf={props.labelOf} />
                      </td>
                      <td class={CELL}>
                        <LinkText link={link()} />
                      </td>
                      <td class={CELL}>
                        <Health link={link()} />
                      </td>
                      <td class={`${CELL} tabular-nums`}>
                        <Show when={link().edge.drops.at(-1)} keyed fallback={t.links.noDrops}>
                          {(last) => td.dropCount(link().edge.drops.length, formatTime(last))}
                        </Show>
                      </td>
                      <td class="py-2">
                        <Show when={link().edge.medium === "mesh"} fallback={NONE}>
                          <Sparkline
                            values={history(link().edge.id)}
                            max={100}
                            label={t.links.historyOf(props.labelOf(link().peer))}
                            class="text-accent"
                          />
                        </Show>
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </div>
    </Card>
  );
}

function LinkText(props: { link: MeshLink }) {
  const edge = () => props.link.edge;
  const medium = () => {
    const text = strings.topology.medium[edge().medium];
    return edge().band ? `${text} ${strings.topology.band[edge().band ?? "5g"]}` : text;
  };
  const ports = () =>
    props.link.port || props.link.peerPort
      ? `${props.link.port ?? NONE} ↔ ${props.link.peerPort ?? NONE}`
      : null;
  return (
    <div class="flex flex-col">
      <span>{medium()}</span>
      <Show when={ports()}>
        {(text) => <span class="font-mono text-fg-muted text-xs">{text()}</span>}
      </Show>
    </div>
  );
}

function Health(props: { link: MeshLink }) {
  const edge = () => props.link.edge;
  const score = () => {
    const value = edge().score ?? edge().quality;
    if (value === null) return null;
    const factor = weakestFactor(edge());
    return td.score(value, factor && td.weakest[factor]);
  };
  return (
    <div class="flex flex-col items-start gap-1">
      <div class="flex flex-wrap gap-1.5">
        <Badge tone={edge().up ? "ok" : "crit"}>{edge().up ? td.up : td.down}</Badge>
        <Show when={edge().critical}>
          <span title={td.criticalHint}>
            <Badge tone="warn" icon={ShieldAlert}>
              {td.critical}
            </Badge>
          </span>
        </Show>
      </div>
      <Show when={score()}>
        {(text) => <span class={scoreClass(edge().score ?? edge().quality ?? 100)}>{text()}</span>}
      </Show>
      <Show when={edge().tq !== null}>
        <span class="text-fg-muted text-xs">
          {t.links.tq(formatPercent(((edge().tq ?? 0) / 255) * 100))}
        </span>
      </Show>
    </div>
  );
}

function FailuresCard(props: { node: number; names: ReadonlyMap<number, string> }) {
  const session = useContext(SessionContext);
  const admin = () => session.user.role === "admin";
  const keyOf = (row: NodeFailure) => `${row.mine.port}~${row.peer.node}~${row.peer.port}`;
  const failures = useQuery(() => ({
    queryKey: ["backhaul-failures"],
    queryFn: ({ signal }) => api.backhaul.failures(signal),
  }));
  /** The cables forgotten here, hidden until the list's refetch drops them. */
  const [forgotten, setForgotten] = createSignal<ReadonlySet<string>>(new Set());
  const rows = createMemo(() =>
    failuresOf(failures.isSuccess ? failures.data : [], props.node).filter(
      (row) => !forgotten().has(keyOf(row)),
    ),
  );
  const [busy, setBusy] = createSignal<string>();
  const [error, setError] = createSignal<string>();
  const [done, setDone] = createSignal(false);
  const peerName = (node: number) => props.names.get(node) ?? `#${node}`;
  const forget = async (row: NodeFailure) => {
    setBusy(keyOf(row));
    setError(undefined);
    setDone(false);
    try {
      await api.backhaul.forget(row.failure.ends);
      setForgotten((now) => new Set(now).add(keyOf(row)));
      setDone(true);
      void failures.refetch();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(undefined);
    }
  };
  return (
    <Show when={rows().length > 0 || done() || error()}>
      <Card aria-label={t.failures.title}>
        <CardHeader title={t.failures.title} description={t.failures.description} />
        <div class="flex flex-col gap-3 px-4 pb-4">
          <RequestError text={error()} />
          <Show when={done()}>
            <p role="status" class="text-fg-muted text-sm">
              {t.failures.forgotten}
            </p>
          </Show>
          <Show when={rows().length > 0}>
            <div class="overflow-x-auto">
              <table class="w-full text-sm" aria-label={t.failures.label}>
                <thead class="text-left text-fg-muted text-xs">
                  <tr>
                    <th class={HEAD}>{t.failures.cable}</th>
                    <th class={HEAD}>{t.failures.why}</th>
                    <th class={HEAD}>{t.failures.when}</th>
                    <th class="py-1.5" />
                  </tr>
                </thead>
                <tbody class="divide-y divide-edge">
                  <For each={rows()}>
                    {(row) => (
                      <tr class="align-top" aria-label={row.mine.port}>
                        <td class={CELL}>
                          <span class="font-mono text-xs">{row.mine.port}</span>
                          {" ↔ "}
                          <a
                            href={`/devices/${row.peer.node}?tab=mesh`}
                            class="rounded-control text-accent hover:underline"
                          >
                            {peerName(row.peer.node)}
                          </a>{" "}
                          <span class="font-mono text-fg-muted text-xs">{row.peer.port}</span>
                        </td>
                        <td class={CELL}>{row.failure.why}</td>
                        <td class={`${CELL} whitespace-nowrap`}>
                          {formatDateTime(row.failure.at)}
                        </td>
                        <td class="py-2 text-right">
                          <Show when={admin()}>
                            <Button
                              size="sm"
                              variant="secondary"
                              disabled={busy() !== undefined}
                              aria-label={t.failures.forgetLabel(
                                row.mine.port,
                                peerName(row.peer.node),
                              )}
                              onClick={() => void forget(row)}
                            >
                              {busy() === keyOf(row) ? `${t.failures.forget}…` : t.failures.forget}
                            </Button>
                          </Show>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>
        </div>
      </Card>
    </Show>
  );
}
