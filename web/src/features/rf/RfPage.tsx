import { useSearchParams } from "@solidjs/router";
import { RadioTower } from "lucide";
import { createEffect, createMemo, createSignal, onSettled, Show, useContext } from "solid-js";
import type { Band } from "../../api/gen/Band";
import { LiveContext } from "../../lib/live";
import { BANDS, createRfView } from "../../lib/rf";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Segmented } from "../../ui/Segmented";
import { Channels } from "./Channels";
import { Matrix } from "./Matrix";
import { Networks } from "./Networks";
import { PlanCard } from "./Plan";
import { RadioCards } from "./RadioCards";
import { Scanning, startScans } from "./Scanning";
import { Spectrum } from "./Spectrum";
import { Utilization } from "./Utilization";

const t = strings.rf;

/**
 * `/insights/rf` (features/wifi.md, RF insight): per band (named in the URL, `?band=2g`), the
 * spectrum of every network heard, the site's radios with who shares their channels, each
 * channel's occupancy, how the devices hear each other, utilization over time and the networks
 * around; admins start scans and set the background schedule.
 */
export function RfPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const [params, setParams] = useSearchParams<{ band?: string }>();
  const view = createRfView(live);
  const admin = () => session.user.role === "admin";
  const [now, setNow] = createSignal(Date.now());
  const [selected, setSelected] = createSignal<string>();
  const names = createMemo(() => new Map(live.fleet.nodes.map((n) => [n.id, n.name])));
  const nodeName = (id: number) => names().get(id) ?? strings.metrics.node(String(id));
  const bands = createMemo(() =>
    BANDS.filter((band) => view()?.report.bands.some((b) => b.band === band)),
  );
  const band = (): Band | undefined => {
    const wanted = params.band as Band | undefined;
    if (wanted && bands().includes(wanted)) return wanted;
    return bands().includes("5g") ? "5g" : bands()[0];
  };
  const report = () => view()?.report.bands.find((b) => b.band === band());
  const scans = () => (view()?.radios ?? []).filter((r) => r.band === band());

  onSettled(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  });
  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-7xl flex-col gap-4">
      <header class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
          <p class="mt-1 text-fg-muted text-sm">{strings.nav.rf.description}</p>
        </div>
        <Show when={view()}>{(v) => <Scanning view={v()} admin={admin()} />}</Show>
      </header>
      <Show
        when={view()}
        fallback={
          <p role="status" class="py-20 text-center text-fg-subtle text-sm">
            {`${t.loading}…`}
          </p>
        }
      >
        <Show
          when={report()}
          fallback={
            <EmptyState icon={RadioTower} title={t.dashboard.empty} description={t.noBand} />
          }
        >
          {(b) => (
            <>
              <Show when={bands().length > 1}>
                <Segmented
                  class="self-start"
                  label={t.band}
                  value={band() ?? "5g"}
                  onChange={(value) => {
                    setSelected(undefined);
                    setParams({ band: value === "5g" ? undefined : value });
                  }}
                  options={bands().map((value) => ({
                    value,
                    label: strings.device.radios.band[value],
                  }))}
                />
              </Show>
              <Card class="min-w-0" aria-label={t.spectrum.title}>
                <CardHeader title={t.spectrum.title} description={t.spectrum.description} />
                <div class="px-4 pb-4">
                  <Show
                    when={b().networks.length > 0}
                    fallback={
                      <p class="py-10 text-center text-fg-subtle text-sm">{t.spectrum.empty}</p>
                    }
                  >
                    <Spectrum
                      band={b()}
                      nodeName={nodeName}
                      selected={selected()}
                      onSelect={setSelected}
                    />
                  </Show>
                </div>
              </Card>
              <RadioCards
                band={b()}
                scans={scans()}
                nodeName={nodeName}
                admin={admin()}
                now={now()}
                onScan={(node, radio) => void startScans(node, radio)}
                onSelect={setSelected}
              />
              <Show when={view()}>
                {(v) => (
                  <PlanCard
                    band={b().band}
                    settings={v().settings}
                    admin={admin()}
                    nodeName={nodeName}
                  />
                )}
              </Show>
              <div class="grid min-w-0 items-start gap-4 xl:grid-cols-[3fr_2fr]">
                <Channels band={b()} nodeName={nodeName} now={now()} />
                <Matrix band={b()} nodeName={nodeName} />
              </div>
              <Utilization band={b()} nodeName={nodeName} />
              <Networks
                band={b()}
                nodeName={nodeName}
                selected={selected()}
                onSelect={setSelected}
              />
            </>
          )}
        </Show>
      </Show>
    </div>
  );
}
