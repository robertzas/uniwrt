import { useQuery } from "@tanstack/solid-query";
import { createSignal, Show, useContext } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import { api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { RequestError } from "../../ui/JobOutcome";
import { TailscaleCard } from "./TailscaleCard";
import { TailscaleLoginDialog } from "./TailscaleLoginDialog";
import { TailscalePeers } from "./TailscalePeers";

/**
 * The gateway's Tailscale on `/vpn` (network.md, VPN; D-140): its card and, while it runs, the
 * tailnet's devices. The status is read only once Tailscale is set up.
 */
export function TailscaleSection(props: { view: SiteView; admin: boolean; onEdit: () => void }) {
  return (
    <Show
      when={props.view.config.vpn.tailscale}
      fallback={
        <TailscaleCard
          view={props.view}
          admin={props.admin}
          status={undefined}
          onEdit={() => props.onEdit()}
        />
      }
    >
      <Live view={props.view} admin={props.admin} onEdit={() => props.onEdit()} />
    </Show>
  );
}

/** The card with what the gateway reports (its `services` set, collected every minute). */
function Live(props: { view: SiteView; admin: boolean; onEdit: () => void }) {
  const live = useContext(LiveContext);
  /** The revision the gateway holds: an apply installs the package or changes the prefs. */
  const applied = () => {
    const id = live.fleet.nodes.find((node) => node.role === "gateway")?.id;
    return id === undefined ? undefined : live.config.applied[id]?.revision;
  };
  const [login, setLogin] = createSignal(false);
  const status = useQuery(() => ({
    queryKey: ["vpn", "tailscale", props.view.revision.id, applied()],
    queryFn: ({ signal }) => api.vpn.tailscale.status(signal),
    // Faster while a login waits, to show it done.
    refetchInterval: login() ? 5_000 : 30_000,
  }));
  const running = () => {
    const daemon = status.data?.daemon;
    return daemon?.state === "running" ? daemon : undefined;
  };
  return (
    <>
      <TailscaleCard
        view={props.view}
        admin={props.admin}
        status={status.data}
        onEdit={() => props.onEdit()}
        onLogin={() => setLogin(true)}
        onChanged={() => void status.refetch()}
      />
      <Show when={running()}>{(daemon) => <TailscalePeers daemon={daemon()} />}</Show>
      <RequestError text={status.isError ? status.error?.message : undefined} />
      <Show when={login()}>
        <TailscaleLoginDialog
          daemon={status.data?.daemon ?? null}
          onClose={() => {
            setLogin(false);
            void status.refetch();
          }}
        />
      </Show>
    </>
  );
}
