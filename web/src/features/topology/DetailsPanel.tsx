import type { JSX } from "@solidjs/web";
import { ExternalLink, PinOff, Users, X } from "lucide";
import { For, Match, Show, Switch } from "solid-js";
import type { NodeState } from "../../api/gen/NodeState";
import type { TopoEdge } from "../../api/gen/TopoEdge";
import type { TopoNode } from "../../api/gen/TopoNode";
import {
  formatBitrate,
  formatBytes,
  formatDbm,
  formatPercent,
  formatRate,
  NONE,
} from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import type { Scene, SceneLink, SceneVertex } from "./scene";

const t = strings.topology;
const d = t.details;
/** A cluster lists this many of its clients. */
const MEMBERS_SHOWN = 30;

const stateTone: Record<NodeState, Tone> = { online: "ok", degraded: "warn", offline: "crit" };

export interface DetailsProps {
  scene: Scene;
  selected: string;
  pinned: boolean;
  /** Whether the selected vertex's clients are grouped; `undefined` when it has none to group. */
  grouped: boolean | undefined;
  labelOf: (id: string) => string;
  onClose: () => void;
  onSelect: (id: string) => void;
  onUnpin: () => void;
  onToggleGroup: () => void;
}

/** What is known of the selected vertex or link, beside the map. */
export function DetailsPanel(props: DetailsProps) {
  const vertex = () => props.scene.byId.get(props.selected);
  const link = () => props.scene.links.find((l) => l.id === props.selected && l.edge !== null);
  return (
    <Show when={vertex() ?? link()}>
      <section
        aria-label={d.label}
        class="absolute top-3 right-3 flex max-h-[calc(100%-5.5rem)] w-80 flex-col overflow-hidden rounded-card border border-card-edge bg-surface shadow-overlay glass:backdrop-blur-panel max-md:inset-x-3 max-md:top-auto max-md:bottom-16 max-md:w-auto max-md:max-h-[50%]"
      >
        <Switch>
          <Match when={vertex()}>{(v) => <VertexDetails {...props} vertex={v()} />}</Match>
          <Match when={link()}>{(l) => <LinkDetails {...props} link={l()} />}</Match>
        </Switch>
      </section>
    </Show>
  );
}

function Header(props: {
  kind: string;
  title: string;
  onClose: () => void;
  children?: JSX.Element;
}) {
  return (
    <header class="flex items-start gap-2 border-edge border-b px-4 py-3">
      <div class="min-w-0 flex-1">
        <p class="text-fg-subtle text-xs">{props.kind}</p>
        <h2 class="truncate font-semibold text-base tracking-tight" title={props.title}>
          {props.title}
        </h2>
        <Show when={props.children}>
          <div class="mt-2 flex flex-wrap gap-1.5">{props.children}</div>
        </Show>
      </div>
      <Button
        variant="ghost"
        size="sm"
        icon={X}
        aria-label={d.close}
        onClick={() => props.onClose()}
      />
    </header>
  );
}

function Rows(props: { children: JSX.Element }) {
  return (
    <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 px-4 py-3 text-sm">
      {props.children}
    </dl>
  );
}

function Row(props: { label: string; children: JSX.Element; mono?: boolean }) {
  return (
    <>
      <dt class="text-fg-subtle">{props.label}</dt>
      <dd class={["min-w-0 break-words text-right", { "font-mono text-xs leading-5": props.mono }]}>
        {props.children}
      </dd>
    </>
  );
}

function linkOf(scene: Scene, id: string): SceneLink | undefined {
  return scene.links.find((link) => link.target === id && link.edge !== null);
}

function mediumText(edge: TopoEdge): string {
  const medium = t.medium[edge.medium];
  return edge.band ? `${medium} ${t.band[edge.band]}` : medium;
}

function VertexDetails(props: DetailsProps & { vertex: SceneVertex }) {
  const node = () => props.vertex.node;
  return (
    <>
      <Header kind={t.kind[props.vertex.kind]} title={props.vertex.label} onClose={props.onClose}>
        <Show when={node()?.state}>
          {(state) => <Badge tone={stateTone[state()]}>{strings.devices.state[state()]}</Badge>}
        </Show>
        <Show when={props.pinned}>
          <Badge tone="info">{d.pinned}</Badge>
        </Show>
      </Header>
      <div class="min-h-0 flex-1 overflow-y-auto">
        <Show when={node()} fallback={<ClusterMembers {...props} />}>
          {(n) => <NodeRows {...props} node={n()} />}
        </Show>
      </div>
      <Show when={props.pinned || props.grouped !== undefined || node()?.node != null}>
        <footer class="flex flex-wrap gap-2 border-edge border-t px-4 py-3">
          <Show when={node()?.node}>
            {(id) => (
              <a
                href={`/devices/${id()}`}
                class="inline-flex h-7 items-center gap-1.5 rounded-control bg-accent-fill px-2.5 font-medium text-on-accent text-xs hover:brightness-110"
              >
                <Icon icon={ExternalLink} class="size-3.5" />
                {d.openDevice}
              </a>
            )}
          </Show>
          <Show when={props.grouped !== undefined}>
            <Button size="sm" icon={Users} onClick={() => props.onToggleGroup()}>
              {props.grouped ? d.expand : d.collapse}
            </Button>
          </Show>
          <Show when={props.pinned}>
            <Button size="sm" variant="ghost" icon={PinOff} onClick={() => props.onUnpin()}>
              {d.unpin}
            </Button>
          </Show>
        </footer>
      </Show>
    </>
  );
}

function NodeRows(props: DetailsProps & { vertex: SceneVertex; node: TopoNode }) {
  const uplink = () => linkOf(props.scene, props.node.id);
  return (
    <Rows>
      <Show when={props.vertex.kind === "client" && props.vertex.host}>
        {(host) => <Row label={d.connectedTo}>{props.labelOf(host())}</Row>}
      </Show>
      <Show when={props.node.ips.length > 0}>
        <Row label={d.addresses} mono>
          <For each={props.node.ips}>{(ip) => <span class="block">{ip}</span>}</For>
        </Row>
      </Show>
      <Show when={props.node.mac}>
        {(mac) => (
          <Row label={d.mac} mono>
            {mac()}
          </Row>
        )}
      </Show>
      <Show when={props.node.cpu_percent !== null}>
        <Row label={d.cpu}>{formatPercent(props.node.cpu_percent)}</Row>
      </Show>
      <Show when={props.node.memory_percent !== null}>
        <Row label={d.memory}>{formatPercent(props.node.memory_percent)}</Row>
      </Show>
      <Show when={uplink()?.edge}>
        {(edge) => (
          <>
            <Show when={props.vertex.kind !== "client"}>
              <Row label={d.uplink}>{d.via(props.labelOf(edge().a), mediumText(edge()))}</Row>
            </Show>
            <Show when={edge().ssid}>{(ssid) => <Row label={d.ssid}>{ssid()}</Row>}</Show>
            <Show when={edge().signal_dbm !== null}>
              <Row label={d.signal}>{formatDbm(edge().signal_dbm)}</Row>
            </Show>
            <Show when={props.vertex.kind === "client"}>
              <Row label={d.link}>{mediumText(edge())}</Row>
              <Traffic edge={edge()} />
            </Show>
          </>
        )}
      </Show>
      <Show when={props.node.hops !== null && props.vertex.kind !== "client"}>
        <Row label={d.hops}>{props.node.hops}</Row>
      </Show>
    </Rows>
  );
}

function ClusterMembers(props: DetailsProps & { vertex: SceneVertex }) {
  const members = () => props.vertex.members.slice(0, MEMBERS_SHOWN);
  const more = () => props.vertex.members.length - members().length;
  return (
    <div class="px-4 py-3">
      <Show when={props.vertex.host}>
        {(host) => (
          <p class="mb-2 text-fg-muted text-sm">
            {d.connectedTo}: {props.labelOf(host())}
          </p>
        )}
      </Show>
      <ul class="flex flex-col">
        <For each={members()}>
          {(id) => (
            <li>
              <button
                type="button"
                class="w-full truncate rounded-control px-2 py-1 text-left text-sm hover:bg-raised"
                onClick={() => props.onSelect(id)}
              >
                {props.labelOf(id)}
              </button>
            </li>
          )}
        </For>
      </ul>
      <Show when={more() > 0}>
        <p class="mt-1 px-2 text-fg-subtle text-xs">{d.more(more())}</p>
      </Show>
    </div>
  );
}

function Traffic(props: { edge: TopoEdge }) {
  return (
    <Show when={props.edge.traffic}>
      {(traffic) => (
        <>
          <Row label={`${d.traffic} · ${d.downstream}`}>{formatBitrate(traffic().down_bps)}</Row>
          <Row label={`${d.traffic} · ${d.upstream}`}>{formatBitrate(traffic().up_bps)}</Row>
          <Row label={d.carried}>
            {formatBytes(traffic().down_bytes)} ↓ · {formatBytes(traffic().up_bytes)} ↑
          </Row>
        </>
      )}
    </Show>
  );
}

function LinkDetails(props: DetailsProps & { link: SceneLink }) {
  // `link()` only matches links that have an edge.
  const edge = () => props.link.edge as TopoEdge;
  const ports = () =>
    [edge().a_port, edge().b_port].some((port) => port !== null)
      ? `${edge().a_port ?? NONE} ↔ ${edge().b_port ?? NONE}`
      : undefined;
  return (
    <>
      <Header
        kind={mediumText(edge())}
        title={`${props.labelOf(edge().a)} ↔ ${props.labelOf(edge().b)}`}
        onClose={props.onClose}
      >
        <Badge tone={edge().up ? "ok" : "crit"}>{edge().up ? d.up : d.down}</Badge>
      </Header>
      <div class="min-h-0 flex-1 overflow-y-auto">
        <Rows>
          <Show when={ports()}>
            {(text) => (
              <Row label={d.ports} mono>
                {text()}
              </Row>
            )}
          </Show>
          <Show when={edge().ssid}>{(ssid) => <Row label={d.ssid}>{ssid()}</Row>}</Show>
          <Show when={edge().signal_dbm !== null}>
            <Row label={d.signal}>{formatDbm(edge().signal_dbm)}</Row>
          </Show>
          <Show when={edge().tq !== null}>
            <Row label={d.tq}>{formatPercent(((edge().tq ?? 0) / 255) * 100)}</Row>
          </Show>
          <Show when={edge().quality !== null}>
            <Row label={d.quality}>{edge().quality} / 100</Row>
          </Show>
          <Show when={edge().capacity_mbps !== null}>
            <Row label={d.capacity}>{formatRate((edge().capacity_mbps ?? 0) * 1000)}</Row>
          </Show>
          <Traffic edge={edge()} />
        </Rows>
      </div>
    </>
  );
}
