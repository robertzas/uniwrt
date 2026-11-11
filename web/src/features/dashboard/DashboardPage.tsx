import { LayoutDashboard } from "lucide";
import { createEffect, createMemo, Show, useContext } from "solid-js";
import { createClientsView } from "../../lib/clientList";
import { clientName } from "../../lib/clients";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { EmptyState } from "../../ui/EmptyState";
import { createLiveGraph } from "../../viz/topology/graph";
import { ExperienceCard } from "./ExperienceCard";
import { HealthHeader } from "./HealthHeader";
import { gatewayOf, internetHealth } from "./health";
import { MiniMap } from "./MiniMap";
import { RecentEvents } from "./RecentEvents";
import { RfCard } from "./RfCard";
import { TopClients } from "./TopClients";
import { WanCard } from "./WanCard";

const t = strings.dashboard;

const PRIMARY_LINK =
  "inline-flex h-8 items-center gap-2 rounded-control bg-accent-fill px-3 font-medium text-on-accent text-sm hover:brightness-110";

/**
 * `/`: the network at a glance (features/observability.md, Dashboard): the health header, the
 * internet connection, the topology mini-map, the busiest clients, the Wi-Fi experience, the
 * recent events and the RF summary. Every part is live; only the WAN's history is read from
 * storage.
 */
export function DashboardPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const graph = createLiveGraph(live);
  const clients = createClientsView(live);
  const names = createMemo(() => new Map(live.fleet.nodes.map((node) => [node.id, node.name])));
  const nodeName = (id: number) => names().get(id) ?? `#${id}`;
  const clientNames = createMemo(
    () => new Map(clients.clients.map((client) => [client.mac, clientName(client)])),
  );
  const clientLabel = (mac: string) => clientNames().get(mac) ?? mac;
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
          <div class="mt-6 flex flex-wrap items-center justify-center gap-x-4 gap-y-3">
            <Show
              when={session.user.role === "admin"}
              fallback={
                <a href="/devices?adopt=1" class={PRIMARY_LINK}>
                  {t.welcome.adopt}
                </a>
              }
            >
              <a href="/setup" class={PRIMARY_LINK}>
                {t.welcome.setup}
              </a>
              <a href="/devices?adopt=1" class="text-accent text-sm hover:underline">
                {t.welcome.adoptExisting}
              </a>
            </Show>
          </div>
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
          <RecentEvents live={live} nodeName={nodeName} clientName={clientLabel} />
        </div>
        <RfCard live={live} nodeName={nodeName} />
      </div>
    </Show>
  );
}
