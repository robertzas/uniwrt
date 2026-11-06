import { useParams } from "@solidjs/router";
import { ArrowLeft, Ban, BellRing, Info, Pencil, Pin, Users, UserX } from "lucide";
import { createEffect, createSignal, Match, Show, Switch, useContext } from "solid-js";
import type { ClientDetail } from "../../api/gen/ClientDetail";
import { clientName, kindIcons } from "../../lib/clients";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { Tab, TabList, TabPanel, Tabs } from "../../ui/Tabs";
import { Tooltip } from "../../ui/Tooltip";
import { ClientOverview } from "./ClientOverview";
import { ClientTimeline } from "./ClientTimeline";
import { FixIpDialog, ipv4Of } from "./FixIpDialog";
import { RenameDialog } from "./RenameDialog";
import { SignalTab } from "./SignalTab";
import { createClientView } from "./store";
import { TrafficTab } from "./TrafficTab";

const d = strings.client;

/** `/clients/:mac`: one client, live: Overview · Timeline · Signal · Traffic (ui.md, client page tabs). */
export function ClientPage() {
  const params = useParams<{ mac: string }>();
  const mac = () => decodeURIComponent(params.mac).toLowerCase();
  return (
    <div class="mx-auto max-w-7xl">
      <a
        href="/clients"
        class="mb-4 inline-flex items-center gap-1.5 rounded-control text-fg-muted text-sm hover:text-fg"
      >
        <Icon icon={ArrowLeft} class="size-4" />
        {d.back}
      </a>
      <Show when={/^([0-9a-f]{2}:){5}[0-9a-f]{2}$/.test(mac())} fallback={<NotFound />}>
        <LiveClient mac={mac()} />
      </Show>
    </div>
  );
}

function NotFound() {
  return <EmptyState icon={Users} title={d.notFound.title} description={d.notFound.description} />;
}

function LiveClient(props: { mac: string }) {
  const live = useContext(LiveContext);
  const view = createClientView(live, () => props.mac);
  createEffect(
    () => (view.detail ? clientName(view.detail.client) : strings.clients.title),
    (name) => {
      document.title = `${name} · ${strings.appName}`;
    },
  );
  return (
    <Switch>
      <Match when={!view.ready}>
        <p role="status" class="py-20 text-center text-fg-subtle text-sm">
          {d.loading}…
        </p>
      </Match>
      <Match when={view.detail}>{(detail) => <Client detail={detail()} />}</Match>
      <Match when={view.missing}>
        <NotFound />
      </Match>
    </Switch>
  );
}

function Client(props: { detail: ClientDetail }) {
  const session = useContext(SessionContext);
  const [renaming, setRenaming] = createSignal(false);
  const [fixing, setFixing] = createSignal(false);
  const client = () => props.detail.client;
  const subtitle = () =>
    [
      client().vendor,
      strings.clients.kind[client().kind],
      clientName(client()) === client().mac ? null : client().mac,
      ...client().ips,
    ]
      .filter(Boolean)
      .join(" · ");
  return (
    <div class="flex flex-col gap-4">
      <header class="flex flex-wrap items-start gap-x-4 gap-y-3">
        <span class="grid size-12 shrink-0 place-items-center rounded-card border border-edge bg-surface">
          <Icon icon={kindIcons[client().kind]} class="size-6 text-fg-muted" />
        </span>
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2.5">
            <h1 class="truncate font-semibold text-2xl tracking-tight">{clientName(client())}</h1>
            <Badge tone={client().online ? "ok" : "neutral"}>
              {client().online ? d.online : d.offline}
            </Badge>
            <Show when={client().randomized}>
              <span title={strings.clients.privateHint}>
                <Badge tone="info">{strings.clients.private}</Badge>
              </span>
            </Show>
          </div>
          <p class="mt-1 truncate text-fg-muted text-sm">{subtitle()}</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <a
            href={`/insights/events?clients=${encodeURIComponent(client().mac)}`}
            class="inline-flex h-8 items-center gap-2 rounded-control border border-edge bg-surface px-3 font-medium text-fg text-sm hover:bg-raised"
          >
            <Icon icon={BellRing} class="size-4" />
            {d.events}
          </a>
          <Show when={session.user.role === "admin"}>
            <Button size="sm" icon={Pencil} onClick={() => setRenaming(true)}>
              {d.rename}
            </Button>
            <Show when={ipv4Of(client())}>
              <Button size="sm" icon={Pin} onClick={() => setFixing(true)}>
                {d.fixIp.action}
              </Button>
            </Show>
          </Show>
          <Button size="sm" variant="ghost" icon={UserX} disabled>
            {d.kick}
          </Button>
          <Button size="sm" variant="ghost" icon={Ban} disabled>
            {d.block}
          </Button>
          <Tooltip
            content={d.actionsNeedManagement}
            aria-label={d.actionsUnavailable}
            class="grid size-8 place-items-center rounded-control text-fg-subtle hover:text-fg"
          >
            <Icon icon={Info} class="size-4" />
          </Tooltip>
        </div>
      </header>
      <Tabs defaultValue="overview" aria-label={d.tabs.label}>
        <TabList>
          <Tab value="overview">{d.tabs.overview}</Tab>
          <Tab value="timeline">{d.tabs.timeline}</Tab>
          <Tab value="signal">{d.tabs.signal}</Tab>
          <Tab value="traffic">{d.tabs.traffic}</Tab>
        </TabList>
        <TabPanel value="overview">
          <ClientOverview detail={props.detail} />
        </TabPanel>
        <TabPanel value="timeline">
          <ClientTimeline detail={props.detail} />
        </TabPanel>
        <TabPanel value="signal">
          <SignalTab mac={client().mac} samples={props.detail.samples} />
        </TabPanel>
        <TabPanel value="traffic">
          <TrafficTab mac={client().mac} samples={props.detail.samples} />
        </TabPanel>
      </Tabs>
      <Show when={fixing()}>
        <FixIpDialog client={client()} onClose={() => setFixing(false)} />
      </Show>
      <RenameDialog
        open={renaming()}
        onOpenChange={setRenaming}
        mac={client().mac}
        alias={client().alias}
        placeholder={client().hostname ?? client().mac}
      />
    </div>
  );
}
