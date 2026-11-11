import { useNavigate } from "@solidjs/router";
import { useQuery } from "@tanstack/solid-query";
import { Plus, TriangleAlert, Wifi } from "lucide";
import { createEffect, createMemo, createSignal, For, onCleanup, Show, useContext } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { Wlan } from "../../api/gen/Wlan";
import { api } from "../../lib/api";
import { createClientsView } from "../../lib/clientList";
import { countries } from "../../lib/countries";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { JoinDialog } from "./JoinDialog";
import { clientsOn, onNow, scopeText, warningsOf } from "./summary";
import { WlanEditor } from "./WlanEditor";
import { WlanRow } from "./WlanRow";

const t = strings.wifi;

/** What the editor is open on (`wlan` undefined adds one), with the document as it opened. */
type Editing = { wlan: Wlan | undefined; view: SiteView };

/**
 * `/wifi` (wifi.md, WLANs; D-083): the site's WLANs with their security, bands, network,
 * devices, schedule and the clients on them now. Admins edit each in a dialog and save it as the
 * next revision, then review and apply; a QR code joins a phone.
 */
export function WifiPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const navigate = useNavigate();
  const admin = () => session.user.role === "admin";
  const clients = createClientsView(live);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  const [editing, setEditing] = createSignal<Editing>();
  const [joining, setJoining] = createSignal<Wlan>();
  // The schedules' "on now" follows the clock.
  const [now, setNow] = createSignal(new Date());
  const tick = setInterval(() => setNow(new Date()), 30_000);
  onCleanup(() => clearInterval(tick));

  const nodeName = (id: number) =>
    live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;
  const networkName = (id: string) =>
    site.data?.config.networks.find((network) => network.id === id)?.name ?? id;
  const country = createMemo(() => {
    const code = site.data?.config.site.country;
    return code ? (countries().find((c) => c.code === code)?.name ?? code) : undefined;
  });
  /** The adopted nodes that don't manage `wireless`: applies leave their Wi-Fi alone. */
  const unmanaged = createMemo(() => {
    const nodes = site.data?.config.nodes;
    if (!nodes) return [];
    return live.fleet.nodes.filter(
      (node) => !nodes.find((config) => config.node === node.id)?.managed.includes("wireless"),
    );
  });

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
      <Show when={site.data && site.data.config.site.country === null}>
        <Note text={t.noCountry} action={t.setCountry} onAction={() => navigate("/settings")} />
      </Show>
      <Show when={unmanaged().length > 0 && (site.data?.config.wlans.length ?? 0) > 0}>
        <Note
          text={t.unmanaged(
            unmanaged()
              .map((node) => node.name)
              .join(", "),
          )}
          action={t.devices}
          onAction={() => navigate("/devices")}
        />
      </Show>
      <Card aria-label={t.title}>
        <CardHeader
          title={t.title}
          actions={
            <Show when={admin() && site.isSuccess}>
              <Button
                size="sm"
                icon={Plus}
                onClick={() => site.data && setEditing({ wlan: undefined, view: site.data })}
              >
                {t.add}
              </Button>
            </Show>
          }
        />
        <Show
          when={site.data}
          fallback={
            <p role="status" class="px-4 pb-4 text-fg-muted text-sm">
              {site.isError ? "" : `${t.loading}…`}
            </p>
          }
        >
          {(view) => (
            <Show
              when={view().config.wlans.length > 0}
              fallback={
                <EmptyState icon={Wifi} title={t.empty.title} description={t.empty.description} />
              }
            >
              <ul class="divide-y divide-edge border-edge border-t">
                <For each={view().config.wlans}>
                  {(wlan) => (
                    <WlanRow
                      wlan={wlan}
                      networkName={networkName(wlan.network)}
                      scope={scopeText(wlan, nodeName)}
                      clients={clientsOn(wlan, clients.clients)}
                      on={
                        wlan.schedule
                          ? onNow(wlan.schedule, view().config.site.timezone, now())
                          : undefined
                      }
                      warnings={warningsOf(view().issues, wlan.id)}
                      admin={admin()}
                      onEdit={() => setEditing({ wlan, view: view() })}
                      onJoin={() => setJoining(wlan)}
                    />
                  )}
                </For>
              </ul>
            </Show>
          )}
        </Show>
        <div class="px-4 pt-3 pb-4">
          <RequestError text={site.error?.message} />
          <p class="text-fg-subtle text-xs">
            {[country() ? t.country(country() ?? "") : "", admin() ? "" : t.adminOnly]
              .filter(Boolean)
              .join(" ")}
          </p>
        </div>
      </Card>
      <Show when={editing()}>
        {(open) => (
          <WlanEditor
            view={open().view}
            wlan={open().wlan}
            nodes={live.fleet.nodes}
            onClose={() => setEditing(undefined)}
          />
        )}
      </Show>
      <Show when={joining()}>
        {(wlan) => <JoinDialog wlan={wlan()} onClose={() => setJoining(undefined)} />}
      </Show>
    </div>
  );
}

/** A warning above the list with what to do about it. */
function Note(props: { text: string; action: string; onAction: () => void }) {
  return (
    <div class="flex flex-wrap items-center gap-3 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
      <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
      <p class="min-w-0 flex-1">{props.text}</p>
      <Button size="sm" variant="secondary" onClick={() => props.onAction()}>
        {props.action}
      </Button>
    </div>
  );
}
