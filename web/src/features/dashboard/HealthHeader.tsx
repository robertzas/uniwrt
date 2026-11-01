import type { JSX } from "@solidjs/web";
import { Globe, Laptop, Router, Waypoints, Wifi } from "lucide";
import type { Client } from "../../api/gen/Client";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { formatBitrate, NONE } from "../../lib/format";
import { strings } from "../../lib/strings";
import { StatusDot } from "../../ui/Badge";
import { Icon, type IconNode } from "../../ui/Icon";
import type { Graph } from "../../viz/topology/graph";
import {
  clientsHealth,
  gatewayOf,
  internetHealth,
  meshHealth,
  nodesHealth,
  type Tone,
  wifiHealth,
} from "./health";

const t = strings.dashboard.health;

const tints: Record<Tone, string> = {
  ok: "bg-ok/10 text-ok",
  warn: "bg-warn/10 text-warn",
  crit: "bg-crit/10 text-crit",
  neutral: "bg-raised text-fg-subtle",
};

/** One health tile: a link to where the details are. The tone colors the icon and the dot. */
function Tile(props: {
  href: string;
  icon: IconNode;
  label: string;
  tone: Tone;
  value: JSX.Element;
  detail: JSX.Element;
}) {
  return (
    <a
      href={props.href}
      class="flex min-w-0 items-start gap-3 rounded-card border border-card-edge bg-surface px-3.5 py-3 shadow-card transition-colors hover:border-edge-strong focus-visible:outline-2 focus-visible:outline-accent glass:backdrop-blur-panel"
    >
      <span class={["grid size-9 shrink-0 place-items-center rounded-lg", tints[props.tone]]}>
        <Icon icon={props.icon} class="size-4.5" />
      </span>
      <span class="flex min-w-0 flex-col gap-0.5">
        <span class="flex items-center gap-1.5 font-medium text-fg-subtle text-xs">
          <StatusDot tone={props.tone} class="size-1.5" />
          <span class="truncate">{props.label}</span>
        </span>
        <span class="truncate font-semibold text-xl tabular-nums leading-tight tracking-tight">
          {props.value}
        </span>
        <span class="truncate text-fg-subtle text-xs">{props.detail}</span>
      </span>
    </a>
  );
}

/** The health header: the internet, the mesh, Wi-Fi, the nodes and the clients at a glance. */
export function HealthHeader(props: {
  nodes: readonly NodeSummary[];
  graph: Graph | undefined;
  clients: readonly Client[];
}) {
  const internet = () => internetHealth(props.nodes, props.graph);
  const mesh = () => meshHealth(props.graph);
  const wifi = () => wifiHealth(props.clients);
  const nodes = () => nodesHealth(props.nodes);
  const clients = () => clientsHealth(props.clients);

  const internetDetail = () => {
    const now = internet();
    if (now.state === "online") {
      return now.down === null && now.up === null
        ? t.noCounters
        : t.rates(formatBitrate(now.down), formatBitrate(now.up));
    }
    const gateway = gatewayOf(props.nodes);
    if (!gateway) return t.noGateway;
    return gateway.status.state === "offline" ? t.gatewayOffline : t.noRoute;
  };
  const meshDetail = () => {
    const now = mesh();
    if (now.links === 0) return t.noMesh;
    if (now.up < now.links) return t.linksDown(now.links - now.up);
    return now.weakest === null ? t.links(now.links) : t.weakest(now.weakest);
  };

  return (
    <section aria-label={t.label} class="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-5">
      <Tile
        href="/topology"
        icon={Globe}
        label={t.internet}
        tone={internet().tone}
        value={t.state[internet().state]}
        detail={internetDetail()}
      />
      <Tile
        href="/topology?view=rf"
        icon={Waypoints}
        label={t.mesh}
        tone={mesh().tone}
        value={
          mesh().links === 0
            ? NONE
            : mesh().up < mesh().links
              ? t.linksUp(mesh().up, mesh().links)
              : t.links(mesh().links)
        }
        detail={meshDetail()}
      />
      <Tile
        href="/clients"
        icon={Wifi}
        label={t.wifi}
        tone={wifi().tone}
        value={wifi().score ?? NONE}
        detail={wifi().clients === 0 ? t.noWifi : t.wifiDetail(wifi().clients, wifi().weak)}
      />
      <Tile
        href="/devices"
        icon={Router}
        label={t.nodes}
        tone={nodes().tone}
        value={nodes().total === 0 ? NONE : t.nodesValue(nodes().online, nodes().total)}
        detail={
          nodes().total === 0
            ? t.noNodes
            : nodes().online === nodes().total
              ? t.allOnline
              : t.nodesDetail(nodes().degraded, nodes().offline)
        }
      />
      <Tile
        href="/clients"
        icon={Laptop}
        label={t.clients}
        tone={clients().online > 0 ? "ok" : "neutral"}
        value={clients().online}
        detail={strings.clients.stat.onlineDetail(clients().wifi, clients().wired)}
      />
    </section>
  );
}
