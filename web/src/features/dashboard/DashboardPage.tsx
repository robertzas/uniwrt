import { BellRing, LayoutDashboard } from "lucide";
import { createEffect, createMemo, Show, useContext } from "solid-js";
import { createClientsView } from "../../lib/clientList";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { createLiveGraph } from "../../viz/topology/graph";
import { ExperienceCard } from "./ExperienceCard";
import { HealthHeader } from "./HealthHeader";
import { gatewayOf, internetHealth } from "./health";
import { MiniMap } from "./MiniMap";
import { TopClients } from "./TopClients";
import { WanCard } from "./WanCard";

const t = strings.dashboard;

/**
 * `/`: the network at a glance (features/observability.md, Dashboard): the health header, the
 * internet connection, the topology mini-map, the busiest clients, the Wi-Fi experience and,
 * from M2.5, recent events. Every part is live; only the WAN's history is read from storage.
 */
export function DashboardPage() {
  const live = useContext(LiveContext);
  const graph = createLiveGraph(live);
  const clients = createClientsView(live);
  const names = createMemo(() => new Map(live.fleet.nodes.map((node) => [node.id, node.name])));
  const nodeName = (id: number) => names().get(id) ?? `#${id}`;
  const internet = createMemo(() => internetHealth(live.fleet.nodes, graph()));

  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <Show
      when={!live.fleet.ready || live.fleet.nodes.length > 0}
      fallback={
        <EmptyState
          icon={LayoutDashboard}
          title={t.welcome.title}
          description={t.welcome.description}
        >
          <a
            href="/devices?adopt=1"
            class="mt-6 inline-flex h-8 items-center gap-2 rounded-control bg-accent-fill px-3 font-medium text-on-accent text-sm hover:brightness-110"
          >
            {t.welcome.adopt}
          </a>
        </EmptyState>
      }
    >
      <div class="mx-auto flex max-w-7xl flex-col gap-4">
        <HealthHeader nodes={live.fleet.nodes} graph={graph()} clients={clients.clients} />
        <div class="grid gap-4 lg:grid-cols-5">
          <div class="grid min-w-0 lg:col-span-2">
            <WanCard gateway={gatewayOf(live.fleet.nodes)?.id} internet={internet()} />
          </div>
          <div class="grid min-w-0 lg:col-span-3">
            <MiniMap graph={graph()} nodes={live.fleet.nodes} />
          </div>
        </div>
        <div class="grid gap-4 lg:grid-cols-3">
          <TopClients clients={clients.clients} nodeName={nodeName} />
          <ExperienceCard clients={clients.clients} nodeName={nodeName} />
          <Card class="flex min-w-0 flex-col">
            <CardHeader title={t.events.title} />
            <div class="flex flex-1 flex-col items-center justify-center gap-3 px-6 pb-6 text-center">
              <span class="grid size-10 place-items-center rounded-xl border border-edge bg-raised text-fg-subtle">
                <Icon icon={BellRing} class="size-4.5" />
              </span>
              <p class="max-w-xs text-fg-subtle text-sm">{t.events.empty}</p>
            </div>
          </Card>
        </div>
      </div>
    </Show>
  );
}
