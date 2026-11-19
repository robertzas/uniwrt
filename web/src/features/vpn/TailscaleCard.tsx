import { ExternalLink, LogIn, LogOut, Pencil, Plus, TriangleAlert } from "lucide";
import { createSignal, For, Show, useContext } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { TailscaleStatus } from "../../api/gen/TailscaleStatus";
import { api, errorText } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { StatusDot } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { ADMIN_CONSOLE, tailscaleState, unapprovedText } from "./tailscale";

const t = strings.vpn.tailscale;

/**
 * Tailscale on the gateway (network.md, VPN; D-140): its state in the tailnet, the gateway's
 * name and addresses there, what it advertises and what the tailnet's admin still has to
 * approve, and what `tailscaled` warns about. Admins set it up and edit it in a dialog, and log
 * the gateway in (through the URL it hands out) or out.
 */
export function TailscaleCard(props: {
  view: SiteView;
  admin: boolean;
  status: TailscaleStatus | undefined;
  /** Opens the editor, which the page holds: a first save swaps this card. */
  onEdit: () => void;
  onLogin?: () => void;
  /** After a logout, to read the state again. */
  onChanged?: () => void;
}) {
  const live = useContext(LiveContext);
  const gateway = () => live.fleet.nodes.find((node) => node.role === "gateway");
  const tailscale = () => props.view.config.vpn.tailscale;
  const [confirming, setConfirming] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<string>();

  const logout = async () => {
    setBusy(true);
    setFailure(undefined);
    try {
      await api.vpn.tailscale.logout();
      setConfirming(false);
      props.onChanged?.();
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin}>
            <Show
              when={tailscale()}
              fallback={
                <Button size="sm" icon={Plus} onClick={() => props.onEdit()}>
                  {t.setUp}
                </Button>
              }
            >
              <Button size="sm" icon={Pencil} onClick={() => props.onEdit()}>
                {t.edit}
              </Button>
            </Show>
          </Show>
        }
      />
      <Show
        when={tailscale()}
        fallback={
          <p class="px-4 pb-4 text-fg-muted text-sm">{props.admin ? t.none : t.noneViewer}</p>
        }
      >
        {(ts) => {
          const state = () => tailscaleState(ts(), props.status);
          const daemon = () => props.status?.daemon ?? null;
          const collected = () => {
            const at = props.status?.collected_at;
            return at == null ? undefined : formatDuration((Date.now() - at) / 1000);
          };
          const routes = () =>
            ts()
              .routes.map((id) => props.view.config.networks.find((n) => n.id === id)?.name ?? id)
              .join(", ") || t.noRoutes;
          const observed = () => {
            const node = gateway();
            return node && node.mode !== "managed" ? node.name : undefined;
          };
          const prefsPending = () =>
            ts().enabled &&
            props.status !== undefined &&
            daemon() !== null &&
            !props.status.prefs_applied &&
            observed() === undefined;
          return (
            <div class="flex flex-col gap-3 border-edge border-t px-4 py-3">
              <div class="flex flex-wrap items-center gap-2">
                <p class="flex flex-1 flex-wrap items-center gap-2 text-sm">
                  <StatusDot tone={state().tone} />
                  <span>{state().text}</span>
                  <Show when={ts().enabled && collected()}>
                    {(ago) => <span class="text-fg-subtle text-xs">{t.collected(ago())}</span>}
                  </Show>
                </p>
                <Show when={props.admin && ts().enabled && !confirming()}>
                  <Show when={state().login}>
                    <Button
                      size="sm"
                      variant="primary"
                      icon={LogIn}
                      onClick={() => props.onLogin?.()}
                    >
                      {t.login}
                    </Button>
                  </Show>
                  <Show when={state().logout}>
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={LogOut}
                      onClick={() => setConfirming(true)}
                    >
                      {t.logout}
                    </Button>
                  </Show>
                </Show>
              </div>
              <Show when={observed()}>
                {(name) => <p class="text-fg-muted text-xs">{t.observed(name())}</p>}
              </Show>
              <Show when={confirming()}>
                <div
                  class="rounded-control border border-crit/40 bg-crit/10 px-3 py-2.5 text-sm"
                  role="alert"
                >
                  <p>
                    {t.confirmLogout}
                    <Show when={ts().auth_key}> {t.confirmLogoutKey}</Show>
                  </p>
                  <div class="mt-2 flex gap-2">
                    <Button
                      size="sm"
                      variant="primary"
                      disabled={busy()}
                      onClick={() => void logout()}
                    >
                      {t.logout}
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
                      {t.cancel}
                    </Button>
                  </div>
                </div>
              </Show>
              <RequestError text={failure()} />
              <dl class="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-3">
                <Show when={daemon()?.tailnet}>
                  {(tailnet) => (
                    <div class="min-w-0">
                      <dt class="text-fg-subtle text-xs">{t.tailnet}</dt>
                      <dd class="truncate">{tailnet()}</dd>
                    </div>
                  )}
                </Show>
                <div class="min-w-0">
                  <dt class="text-fg-subtle text-xs">{t.machine}</dt>
                  <dd class="truncate font-mono">
                    {daemon()?.dns_name ?? ts().hostname ?? gateway()?.name ?? "—"}
                  </dd>
                </div>
                <Show when={(daemon()?.addresses.length ?? 0) > 0}>
                  <div class="min-w-0">
                    <dt class="text-fg-subtle text-xs">{t.addresses}</dt>
                    <dd class="truncate font-mono" title={daemon()?.addresses.join(", ")}>
                      {daemon()?.addresses.join(", ")}
                    </dd>
                  </div>
                </Show>
                <div class="min-w-0">
                  <dt class="text-fg-subtle text-xs">{t.routes}</dt>
                  <dd class="truncate">{routes()}</dd>
                </div>
                <div>
                  <dt class="text-fg-subtle text-xs">{t.exitNode}</dt>
                  <dd>{t.exitNodeValue(ts().exit_node)}</dd>
                </div>
                <Show when={daemon()?.version}>
                  {(version) => (
                    <div class="min-w-0">
                      <dt class="text-fg-subtle text-xs">{t.version}</dt>
                      <dd class="truncate">{version()}</dd>
                    </div>
                  )}
                </Show>
              </dl>
              <Show when={prefsPending()}>
                <p class="text-fg-muted text-xs">{t.prefsPending}</p>
              </Show>
              <Show when={(props.status?.unapproved_routes.length ?? 0) > 0}>
                <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-warn text-xs">
                  <Icon icon={TriangleAlert} class="size-3.5 shrink-0" />
                  {t.unapproved(unapprovedText(props.status?.unapproved_routes ?? []))}
                  <a
                    class="inline-flex items-center gap-1 text-accent underline-offset-2 hover:underline"
                    href={ADMIN_CONSOLE}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {t.openConsole}
                    <Icon icon={ExternalLink} class="size-3" />
                  </a>
                </p>
              </Show>
              <Show when={(daemon()?.health.length ?? 0) > 0}>
                <ul class="flex flex-col gap-1" aria-label={t.health}>
                  <For each={daemon()?.health ?? []}>
                    {(warning) => (
                      <li class="flex items-start gap-2 text-warn text-xs">
                        <Icon icon={TriangleAlert} class="mt-0.5 size-3.5 shrink-0" />
                        {warning}
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
              <Show when={gateway()?.version?.startsWith("24.")}>
                <p class="text-fg-subtle text-xs">{t.logs}</p>
              </Show>
            </div>
          );
        }}
      </Show>
    </Card>
  );
}
