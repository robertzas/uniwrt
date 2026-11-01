import { useSearchParams } from "@solidjs/router";
import { Activity, ArrowUpRight, ZoomOut } from "lucide";
import { createEffect, createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { Metric } from "../../api/gen/Metric";
import type { Unit } from "../../api/gen/Unit";
import { api } from "../../lib/api";
import { clientName } from "../../lib/clients";
import { formatDateTime } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { ChartToolbar } from "../../viz/timeseries/ChartToolbar";
import { createMetricsFeed, createSeriesListing } from "../../viz/timeseries/feed";
import { MetricChart } from "../../viz/timeseries/MetricChart";
import { isPreset, type Preset, type TimeRange } from "../../viz/timeseries/ranges";
import { METRIC_UNITS, metricLabel } from "../../viz/timeseries/units";
import { EntityList } from "./EntityList";
import { entityLabel, entityPage, groupEntities, type Names, parseEntity } from "./entities";

const m = strings.metrics;
/** The most metrics one query reads (the API's limit). */
const MAX_METRICS = 8;
const DEFAULT_RANGE: Preset = "24h";

type Params = { entity?: string; metrics?: string; range?: string; from?: string; to?: string };

/**
 * `/insights/metrics`: any entity's stored series over a range. Pick an entity, its metrics (up
 * to 8, one chart per unit) and a range, or drag across a chart to zoom in. The choice lives in
 * the URL, so a view can be shared and the device and client pages link here.
 */
export function MetricsPage() {
  const live = useContext(LiveContext);
  const [params, setParams] = useSearchParams<Params>();
  const [filter, setFilter] = createSignal("");
  const listing = createSeriesListing();
  const clients = createClientNames();
  const names: Names = {
    node: (id) => live.fleet.nodes.find((node) => node.id === id)?.name,
    client: (mac) => clients().get(mac),
  };
  const groups = createMemo(() => groupEntities(listing() ?? [], names, filter()));
  const info = createMemo(() => {
    const all = listing() ?? [];
    return (
      all.find((i) => i.entity === params.entity) ?? all.find((i) => i.entity === "site") ?? all[0]
    );
  });
  const entity = createMemo(() => {
    const text = info()?.entity;
    const parsed = text === undefined ? undefined : parseEntity(text);
    return text === undefined || parsed === undefined ? undefined : { text, parsed };
  });
  const chosen = createMemo((): Metric[] => {
    const available = info()?.metrics ?? [];
    const asked = (params.metrics ?? "").split(",");
    const picked = available.filter((metric) => asked.includes(metric));
    return (picked.length > 0 ? picked : available).slice(0, MAX_METRICS);
  });
  const zoomed = createMemo(() => {
    const from = Number(params.from);
    const to = Number(params.to);
    return params.from && params.to && Number.isFinite(from) && Number.isFinite(to) && to > from
      ? { from, to }
      : undefined;
  });
  const preset = (): Preset => (isPreset(params.range) ? params.range : DEFAULT_RANGE);
  const range = (): TimeRange => zoomed() ?? { preset: preset() };
  const feed = createMetricsFeed(() => {
    const text = entity()?.text;
    return text === undefined || chosen().length === 0
      ? undefined
      : { queries: [{ entity: text, metrics: chosen() }], range: range(), width: 1000 };
  });
  /** The chosen metrics, one chart per unit, in the order the entity lists them. */
  const charts = createMemo(() => {
    const byUnit = new Map<Unit, Metric[]>();
    for (const metric of chosen()) {
      const unit = METRIC_UNITS[metric];
      byUnit.set(unit, [...(byUnit.get(unit) ?? []), metric]);
    }
    return [...byUnit.values()];
  });
  const toggle = (metric: Metric) => {
    const now = chosen();
    const next = now.includes(metric) ? now.filter((x) => x !== metric) : [...now, metric];
    if (next.length === 0) return;
    const order = info()?.metrics ?? [];
    setParams({ metrics: order.filter((x) => next.includes(x)).join(",") });
  };
  const zoom = (from: number, to: number) =>
    setParams({ from: String(Math.round(from)), to: String(Math.round(to)) });
  createEffect(
    () => m.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-7xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{m.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{strings.nav.metrics.description}</p>
      </header>
      <Show
        when={listing() !== undefined}
        fallback={
          <p role="status" class="py-20 text-center text-fg-subtle text-sm">
            {m.loading}…
          </p>
        }
      >
        <Show
          when={(listing() ?? []).length > 0}
          fallback={
            <EmptyState icon={Activity} title={m.empty.title} description={m.empty.description} />
          }
        >
          <div class="grid items-start gap-4 lg:grid-cols-[17rem_minmax(0,1fr)]">
            <EntityList
              groups={groups()}
              selected={entity()?.text}
              filter={filter()}
              onFilter={setFilter}
              onSelect={(text) => setParams({ entity: text, metrics: undefined })}
            />
            <div class="flex min-w-0 flex-col gap-4">
              <Card class="flex flex-col gap-3 p-4">
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <Show when={entity()}>
                    {(e) => (
                      <div class="min-w-0">
                        <h2 class="truncate font-semibold text-lg tracking-tight">
                          {entityLabel(e().parsed, names)}
                        </h2>
                        <p class="mt-0.5 flex flex-wrap items-center gap-x-2 text-fg-subtle text-xs">
                          <span>{m.kind[e().parsed.kind]}</span>
                          <span class="font-mono">{e().text}</span>
                          <Show when={entityPage(e().parsed)}>
                            {(href) => (
                              <a
                                href={href()}
                                class="inline-flex items-center gap-0.5 font-medium text-accent hover:underline"
                              >
                                {m.openPage}
                                <Icon icon={ArrowUpRight} class="size-3.5" />
                              </a>
                            )}
                          </Show>
                        </p>
                      </div>
                    )}
                  </Show>
                  <ChartToolbar
                    range={zoomed() ? undefined : preset()}
                    onRange={(next) => setParams({ range: next, from: undefined, to: undefined })}
                    source={feed.resolution()}
                  />
                </div>
                <fieldset class="flex flex-wrap items-center gap-1.5">
                  <legend class="sr-only">
                    {m.choose} ({m.chooseHint})
                  </legend>
                  <For each={info()?.metrics ?? []}>
                    {(metric) => {
                      const on = () => chosen().includes(metric);
                      return (
                        <button
                          type="button"
                          aria-pressed={on() ? "true" : "false"}
                          disabled={!on() && chosen().length >= MAX_METRICS}
                          onClick={() => toggle(metric)}
                          class={[
                            "h-7 rounded-full border px-3 font-medium text-xs transition-colors focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50",
                            on()
                              ? "border-accent/50 bg-accent/10 text-fg"
                              : "border-edge text-fg-subtle hover:text-fg",
                          ]}
                        >
                          {metricLabel(metric)}
                        </button>
                      );
                    }}
                  </For>
                </fieldset>
                <div class="flex flex-wrap items-center gap-2 text-fg-subtle text-xs">
                  <Show when={zoomed()} fallback={<span>{strings.chart.zoomHint}</span>}>
                    {(w) => (
                      <>
                        <span class="tabular-nums">
                          {strings.chart.window(formatDateTime(w().from), formatDateTime(w().to))}
                        </span>
                        <Button
                          size="sm"
                          variant="ghost"
                          icon={ZoomOut}
                          onClick={() => setParams({ from: undefined, to: undefined })}
                        >
                          {strings.chart.resetZoom}
                        </Button>
                      </>
                    )}
                  </Show>
                  <Show when={feed.failed()}>
                    <span role="alert" class="text-crit">
                      {strings.chart.failed}
                    </span>
                  </Show>
                </div>
              </Card>
              <For each={charts()}>
                {(metrics) => (
                  <Card class="p-4">
                    <MetricChart
                      title={metrics.map(metricLabel).join(" · ")}
                      feed={feed}
                      lines={metrics.map((metric) => ({ entity: entity()?.text ?? "", metric }))}
                      height={220}
                      sync="explorer"
                      onZoom={zoom}
                    />
                  </Card>
                )}
              </For>
            </div>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/** Clients by MAC, named as elsewhere in the UI; read once when the page opens. */
function createClientNames(): () => Map<string, string> {
  const [names, setNames] = createSignal(new Map<string, string>());
  onSettled(() => {
    const controller = new AbortController();
    void api.clients
      .list(controller.signal)
      .then((list) => setNames(new Map(list.map((client) => [client.mac, clientName(client)]))))
      .catch(() => {
        // MACs stand in for names.
      });
    return () => controller.abort();
  });
  return names;
}
