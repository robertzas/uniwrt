import { useNavigate } from "@solidjs/router";
import { useQuery } from "@tanstack/solid-query";
import { ShieldCheck, TriangleAlert } from "lucide";
import { createEffect, createMemo, Show, useContext } from "solid-js";
import { api } from "../../lib/api";
import { createClientsView } from "../../lib/clientList";
import { clientName } from "../../lib/clients";
import { ipv4 } from "../../lib/ipv4";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import type { HostChoice } from "./ForwardDialog";
import { ForwardsCard } from "./ForwardsCard";
import { MatrixCard } from "./MatrixCard";
import { UpnpCard } from "./UpnpCard";

const t = strings.security;

/**
 * `/security` (network.md, Firewall policy; D-079): the matrix of which networks reach which,
 * port forwards with the DMZ host, and UPnP with the ports clients opened. Admins save each
 * change as the next revision, then review and apply.
 */
export function SecurityPage() {
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
  // The same query as the device and networks pages', so they share the gateway's facts.
  const gatewayDetail = useQuery(() => ({
    queryKey: ["node", gateway()?.id],
    queryFn: ({ signal }) => api.nodes.get(gateway()?.id ?? 0, signal),
    enabled: gateway() !== undefined,
    refetchInterval: 60_000,
  }));
  const managed = () => {
    const node = gateway();
    return site.data?.config.nodes.find((config) => config.node === node?.id)?.managed;
  };
  /** The gateway, when applies leave its firewall alone. */
  const unmanaged = () => {
    const node = gateway();
    const packages = managed();
    return node && packages && !packages.includes("firewall") ? node : undefined;
  };
  /** The clients' IPv4 addresses with their names, for the forms and the lists. */
  const hosts = createMemo<HostChoice[]>(() =>
    clients.clients.flatMap((client) =>
      client.ips
        .filter((ip) => ipv4(ip) !== undefined)
        .map((ip) => ({ ip, name: clientName(client) })),
    ),
  );
  const hostName = (ip: string) => hosts().find((h) => h.ip === ip)?.name;

  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-5xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{t.description}</p>
      </header>
      <Show when={live.fleet.ready && !gateway()}>
        <EmptyState
          icon={ShieldCheck}
          title={t.noGateway.title}
          description={t.noGateway.description}
        />
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
      <Show
        when={site.data}
        fallback={
          <p role="status" class="text-fg-muted text-sm">
            {site.isError ? "" : `${t.loading}…`}
          </p>
        }
      >
        {(view) => (
          <>
            <MatrixCard view={view()} admin={admin()} />
            <ForwardsCard view={view()} admin={admin()} hosts={hosts()} />
            <UpnpCard
              view={view()}
              admin={admin()}
              mappings={gatewayDetail.data?.facts?.port_mappings ?? []}
              unmanaged={
                gateway() && managed() && !managed()?.includes("upnpd")
                  ? gateway()?.name
                  : undefined
              }
              hostName={hostName}
            />
            <Show when={!admin()}>
              <p class="text-fg-subtle text-xs">{t.adminOnly}</p>
            </Show>
          </>
        )}
      </Show>
      <RequestError text={site.error?.message} />
    </div>
  );
}
