import { useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, onSettled, Show, useContext } from "solid-js";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Tab, TabList, TabPanel, Tabs } from "../../ui/Tabs";
import { ActiveTab } from "./ActiveTab";
import { HistoryTab } from "./HistoryTab";
import { type HistoryQuery, parseHistory } from "./history";
import { MaintenanceTab } from "./MaintenanceTab";
import { RulesTab } from "./RulesTab";
import { createNames } from "./shared";

const t = strings.alerts;

const TABS = ["active", "history", "rules", "maintenance"] as const;
type TabName = (typeof TABS)[number];

interface Params {
  tab?: string;
  /** An alert to open (from the bell). */
  alert?: string;
  /** A rule to edit or mark (from an alert). */
  rule?: string;
  min?: string;
  node?: string;
  range?: string;
  [key: string]: string | undefined;
}

/** A positive integer parameter, or null. */
function idParam(value: string | undefined): number | null {
  const id = Number(value);
  return Number.isInteger(id) && id > 0 ? id : null;
}

/**
 * `/insights/alerts` (features/observability.md, Alerts): the alerts firing, the history, the
 * rules and maintenance windows, in tabs named in the URL (`?tab=history&min=warning&range=30d`).
 */
export function AlertsPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const [params, setParams] = useSearchParams<Params>();
  const names = createNames(live);
  const admin = () => session.user.role === "admin";
  const tab = (): TabName =>
    (TABS as readonly string[]).includes(params.tab ?? "") ? (params.tab as TabName) : "active";
  const history = createMemo(() =>
    parseHistory({ min: params.min, node: params.node, range: params.range }),
  );
  const [expanded, setExpanded] = createSignal<ReadonlySet<number>>(
    new Set([idParam(params.alert)].filter((id): id is number => id !== null)),
  );
  const [now, setNow] = createSignal(Date.now());

  const toggle = (id: number) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const setTab = (value: string) =>
    setParams({
      tab: value === "active" ? undefined : value,
      alert: undefined,
      rule: undefined,
      min: undefined,
      node: undefined,
      range: undefined,
    });
  const setHistory = (query: HistoryQuery) =>
    setParams({
      min: query.min ?? undefined,
      node: query.node === null ? undefined : String(query.node),
      range: query.range === "7d" ? undefined : query.range,
    });

  onSettled(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  });
  // The bell's alert: opened above, and in view once its row is drawn.
  createEffect(
    () => [idParam(params.alert), live.alerts.ready] as const,
    ([id, ready]) => {
      if (id === null || !ready) return;
      setExpanded((current) => (current.has(id) ? current : new Set([...current, id])));
      queueMicrotask(() =>
        document.getElementById(`alert-${id}`)?.scrollIntoView({ block: "nearest" }),
      );
    },
  );
  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-7xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{strings.nav.alerts.description}</p>
      </header>
      <Tabs value={tab()} onChange={setTab} aria-label={t.title}>
        <TabList>
          <Tab value="active">
            <span class="flex items-center gap-2">
              {t.tabs.active}
              <Show when={live.alerts.active.length > 0}>
                <span class="rounded-full bg-raised px-1.5 font-mono text-[0.6875rem] text-fg-muted tabular-nums">
                  {live.alerts.active.length}
                </span>
              </Show>
            </span>
          </Tab>
          <Tab value="history">{t.tabs.history}</Tab>
          <Tab value="rules">{t.tabs.rules}</Tab>
          <Tab value="maintenance">{t.tabs.maintenance}</Tab>
        </TabList>
        <TabPanel value="active">
          <ActiveTab
            live={live}
            names={names}
            now={now()}
            admin={admin()}
            expanded={expanded()}
            onToggle={toggle}
          />
        </TabPanel>
        <TabPanel value="history">
          <HistoryTab
            live={live}
            names={names}
            now={now()}
            admin={admin()}
            query={history()}
            onQuery={setHistory}
            expanded={expanded()}
            onToggle={toggle}
          />
        </TabPanel>
        <TabPanel value="rules">
          <RulesTab
            names={names}
            nodes={live.fleet.nodes}
            admin={admin()}
            focus={idParam(params.rule)}
            onFocusDone={() => setParams({ rule: undefined })}
          />
        </TabPanel>
        <TabPanel value="maintenance">
          <MaintenanceTab nodes={live.fleet.nodes} admin={admin()} now={now()} />
        </TabPanel>
      </Tabs>
    </div>
  );
}
