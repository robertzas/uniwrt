import { useNavigate } from "@solidjs/router";
import { useQuery } from "@tanstack/solid-query";
import { Layers, Plus, TriangleAlert } from "lucide";
import { createEffect, createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { Network } from "../../api/gen/Network";
import type { SiteView } from "../../api/gen/SiteView";
import { api } from "../../lib/api";
import { createClientsView } from "../../lib/clientList";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { sortNetworks } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { DnsCard } from "./DnsCard";
import { NetworkEditor } from "./NetworkEditor";
import { NetworkRow } from "./NetworkRow";
import { NewNetworkDialog } from "./NewNetworkDialog";
import { useOf, warningsOf } from "./summary";

const t = strings.networks;

/**
 * What the editor is open on: a saved network, or a new one from the address plan, with the
 * document as it opened (a save moves the latest revision on while the editor shows its outcome).
 */
type Editing = { network: Network; isNew: boolean; view: SiteView };

/**
 * `/networks` (network.md, Networks; D-077): the site's networks with their subnet, VLAN,
 * purpose, pool use and clients, and its local DNS records. Admins edit each in a dialog and
 * save it as the next revision, then review and apply.
 */
export function NetworksPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const navigate = useNavigate();
  const admin = () => session.user.role === "admin";
  const clients = createClientsView(live);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  const gateway = () => live.fleet.nodes.find((node) => node.role === "gateway");
  // The same query as the device page's, so the two share the gateway's leases.
  const gatewayDetail = useQuery(() => ({
    queryKey: ["node", gateway()?.id],
    queryFn: ({ signal }) => api.nodes.get(gateway()?.id ?? 0, signal),
    enabled: gateway() !== undefined,
    refetchInterval: 60_000,
  }));
  const leases = () => gatewayDetail.data?.facts?.leases ?? [];
  const networks = createMemo(() => sortNetworks(site.data?.config.networks ?? []));
  const [choosing, setChoosing] = createSignal(false);
  const [editing, setEditing] = createSignal<Editing>();
  /** The gateway, when it doesn't manage `network` and `dhcp`: applies leave its LAN alone. */
  const unmanaged = () => {
    const node = gateway();
    const nodes = site.data?.config.nodes;
    if (!node || !nodes) return undefined;
    const managed = nodes.find((config) => config.node === node.id)?.managed ?? [];
    return managed.includes("network") && managed.includes("dhcp") ? undefined : node;
  };

  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-4xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{t.description}</p>
      </header>
      <Show when={live.fleet.ready && !gateway()}>
        <EmptyState icon={Layers} title={t.noGateway.title} description={t.noGateway.description} />
      </Show>
      <Show when={unmanaged()}>
        {(node) => (
          <div class="flex flex-wrap items-center gap-3 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
            <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
            <p class="min-w-0 flex-1">{t.unmanaged(node().name)}</p>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => navigate(`/devices/${node().id}?tab=config`)}
            >
              {t.manage}
            </Button>
          </div>
        )}
      </Show>
      <Card aria-label={t.title}>
        <CardHeader
          title={t.title}
          actions={
            <Show when={admin() && site.isSuccess}>
              <Button size="sm" icon={Plus} onClick={() => setChoosing(true)}>
                {t.add}
              </Button>
            </Show>
          }
        />
        <Show
          when={site.isSuccess}
          fallback={
            <p role="status" class="px-4 pb-4 text-fg-muted text-sm">
              {site.isError ? "" : `${t.loading}…`}
            </p>
          }
        >
          <ul class="divide-y divide-edge border-edge border-t">
            <For each={networks()}>
              {(network) => (
                <NetworkRow
                  network={network}
                  use={useOf(network, leases(), clients.clients)}
                  warnings={warningsOf(site.data?.issues ?? [], network.id)}
                  admin={admin()}
                  onEdit={() => site.data && setEditing({ network, isNew: false, view: site.data })}
                />
              )}
            </For>
          </ul>
        </Show>
        <div class="px-4 pt-3 pb-4">
          <RequestError text={site.error?.message} />
          <p class="text-fg-subtle text-xs">{admin() ? t.ports : t.adminOnly}</p>
        </div>
      </Card>
      <Show when={site.data}>{(view) => <DnsCard view={view()} admin={admin()} />}</Show>
      <NewNetworkDialog
        open={choosing()}
        onOpenChange={setChoosing}
        onChosen={(network) => {
          setChoosing(false);
          if (site.data) setEditing({ network, isNew: true, view: site.data });
        }}
      />
      <Show when={editing()}>
        {(open) => (
          <NetworkEditor
            view={open().view}
            network={open().network}
            isNew={open().isNew}
            nodeName={(id) => live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`}
            onClose={() => setEditing(undefined)}
          />
        )}
      </Show>
    </div>
  );
}
