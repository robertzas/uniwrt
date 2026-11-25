import { useQuery } from "@tanstack/solid-query";
import { GlobeLock } from "lucide";
import { createEffect, createSignal, Show, useContext } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { WireGuard } from "../../api/gen/WireGuard";
import { api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { useNextSteps } from "../../lib/nextStep";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { EmptyState } from "../../ui/EmptyState";
import { RequestError } from "../../ui/JobOutcome";
import { PeersCard } from "./PeersCard";
import { ServerCard } from "./ServerCard";
import { ServerEditor } from "./ServerEditor";
import { TailscaleEditor } from "./TailscaleEditor";
import { TailscaleSection } from "./TailscaleSection";

const t = strings.vpn;

/**
 * `/vpn` (network.md, VPN; D-140): the gateway's WireGuard server and its peers, with what the
 * tunnel reports for each (handshake, traffic), and its Tailscale. Admins set the server up, add
 * and edit peers in dialogs (each save the site's next revision), reveal a peer's configuration
 * as a QR code and a `.conf`, set Tailscale up and log the gateway in or out of the tailnet.
 */
export function VpnPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const next = useNextSteps();
  const admin = () => session.user.role === "admin";
  const gateway = () => live.fleet.nodes.find((node) => node.role === "gateway");
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  // Held here, not by the card: setting the server up swaps the card for one with its status,
  // and the editor stays open to show the saved revision.
  const [editing, setEditing] = createSignal<SiteView>();
  const [tailscale, setTailscale] = createSignal<SiteView>();

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
        <EmptyState
          icon={GlobeLock}
          title={t.noGateway.title}
          description={t.noGateway.description}
          actions={next.gateway()}
        />
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
            <Show
              when={view().config.vpn.wireguard}
              fallback={
                <ServerCard
                  view={view()}
                  admin={admin()}
                  status={undefined}
                  onEdit={() => setEditing(view())}
                />
              }
            >
              {(server) => (
                <WireGuardCards
                  view={view()}
                  server={server()}
                  admin={admin()}
                  onEdit={() => setEditing(view())}
                />
              )}
            </Show>
            <TailscaleSection view={view()} admin={admin()} onEdit={() => setTailscale(view())} />
          </>
        )}
      </Show>
      <RequestError text={site.error?.message} />
      <Show when={editing()}>
        {(view) => <ServerEditor view={view()} onClose={() => setEditing(undefined)} />}
      </Show>
      <Show when={tailscale()}>
        {(view) => <TailscaleEditor view={view()} onClose={() => setTailscale(undefined)} />}
      </Show>
    </div>
  );
}

/** The server and its peers, with what the gateway's tunnel reports (collected every minute). */
function WireGuardCards(props: {
  view: SiteView;
  server: WireGuard;
  admin: boolean;
  onEdit: () => void;
}) {
  const live = useContext(LiveContext);
  /** The revision the gateway holds: an apply changes which peers and key it runs. */
  const applied = () => {
    const id = live.fleet.nodes.find((node) => node.role === "gateway")?.id;
    return id === undefined ? undefined : live.config.applied[id]?.revision;
  };
  const status = useQuery(() => ({
    queryKey: ["vpn", "wireguard", props.view.revision.id, applied()],
    queryFn: ({ signal }) => api.vpn.wireguard.peers(signal),
    refetchInterval: 30_000,
  }));
  return (
    <>
      <ServerCard
        view={props.view}
        admin={props.admin}
        status={status.data}
        onEdit={() => props.onEdit()}
      />
      <PeersCard view={props.view} server={props.server} admin={props.admin} status={status.data} />
      <RequestError text={status.isError ? status.error?.message : undefined} />
    </>
  );
}
