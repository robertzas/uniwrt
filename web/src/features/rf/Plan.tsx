import { createSignal, For, Show } from "solid-js";
import type { Band } from "../../api/gen/Band";
import type { BandPlan } from "../../api/gen/BandPlan";
import type { Breakdown } from "../../api/gen/Breakdown";
import type { DfsPolicy } from "../../api/gen/DfsPolicy";
import type { Plan } from "../../api/gen/Plan";
import type { RadioPlan } from "../../api/gen/RadioPlan";
import type { Reason } from "../../api/gen/Reason";
import type { RfSettings } from "../../api/gen/RfSettings";
import type { Setting } from "../../api/gen/Setting";
import { api, errorText } from "../../lib/api";
import { formatPercent } from "../../lib/format";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { SavedRevision } from "../../ui/SavedRevision";
import { Segmented } from "../../ui/Segmented";
import { type Column, Table } from "../../ui/Table";
import { showToast } from "../../ui/Toast";
import { Nightly } from "./Nightly";
import { usable, withBandPlan } from "./planned";

const t = strings.rf.plan;
const POLICIES: readonly DfsPolicy[] = ["avoid", "non_weather", "all"];
/** The nightly optimizer's threshold (features/wifi.md, RF planner: Apply). */
const WORTH = 0.15;
const TERMS = ["own", "external", "dfs", "change", "width"] as const;

/** A setting in words: channel, width and power. */
export function settingText(setting: Setting): string {
  const power = setting.tx_power_dbm === null ? t.autoPower : `${setting.tx_power_dbm} dBm`;
  return t.setting(setting.channel, setting.width, power);
}

/** Why a radio is planned as it is, in words. */
export function reasonText(reason: Reason, nodeName: (id: number) => string): string {
  const r = t.reason;
  switch (reason.kind) {
    case "mesh_group":
      return r.mesh_group(reason.radios - 1);
    case "pin_conflict":
      return r.pin_conflict(`${nodeName(reason.node)} ${reason.radio}`);
    case "narrowed":
      return r.narrowed(reason.widest);
    case "power_lowered":
      return r.power_lowered(reason.heard_dbm);
    case "power_raised":
      return r.power_raised(reason.heard_dbm);
    case "mesh_power":
      return r.mesh_power(nodeName(reason.peer), reason.signal_dbm);
    default:
      return r[reason.kind];
  }
}

/** The terms that count, such as "neighbors 0.42 · width 0.3". */
function termsText(cost: Breakdown): string {
  return TERMS.filter((term) => cost[term] > 0)
    .map((term) => `${t.terms[term]} ${cost[term].toFixed(2)}`)
    .join(" · ");
}

/**
 * The RF planner's preview for the band (features/wifi.md, RF planner): planned on demand, as
 * planning a large site takes a moment; the band's cost before and after, and each radio's
 * setting now and as planned, its cost term by term and why. Admins set the DFS policy and the
 * nightly optimization here, and save a plan as the site's next revision to review and apply
 * (D-112).
 */
export function PlanCard(props: {
  band: Band;
  settings: RfSettings;
  admin: boolean;
  nodeName: (id: number) => string;
}) {
  const [plan, setPlan] = createSignal<Plan>();
  const [planning, setPlanning] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [using, setUsing] = createSignal(false);
  /** The revision a band's plan was saved as. */
  const [saved, setSaved] = createSignal<{ band: Band; revision: number }>();
  const run = () => {
    setPlanning(true);
    setSaved(undefined);
    api.rf
      .plan()
      .then((answer) => setPlan(() => answer))
      .catch((error: unknown) =>
        showToast({ title: t.failed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setPlanning(false));
  };
  const setDfs = (dfs: DfsPolicy) => {
    setSaving(true);
    api.rf
      .settings({ ...props.settings, dfs })
      .then(() => {
        showToast({ title: t.dfs.saved, tone: "ok" });
        if (plan()) run();
      })
      .catch((error: unknown) =>
        showToast({ title: t.dfs.failed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setSaving(false));
  };
  const banded = () => plan()?.bands.find((b) => b.band === props.band);
  const use = (band: BandPlan) => {
    setUsing(true);
    const changed = band.radios.filter((r) => r.changed).length;
    saveSite(
      (config) => withBandPlan(config, band),
      t.message(strings.device.radios.band[band.band], changed),
    )
      .then((view) => setSaved({ band: band.band, revision: view.revision.id }))
      .catch((error: unknown) =>
        showToast({ title: t.useFailed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setUsing(false));
  };
  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <div class="flex flex-wrap items-center justify-end gap-3 text-sm">
            <Nightly settings={props.settings} admin={props.admin} />
            <Show when={props.band === "5g"}>
              <Show
                when={props.admin}
                fallback={
                  <span class="text-fg-muted text-xs">
                    {`${t.dfs.label}: ${t.dfs[props.settings.dfs]}`}
                  </span>
                }
              >
                <div class="flex items-center gap-2" title={t.dfs.hint}>
                  <span class="text-fg-muted text-xs">{t.dfs.label}</span>
                  <Segmented
                    label={t.dfs.label}
                    value={props.settings.dfs}
                    disabled={saving()}
                    onChange={setDfs}
                    options={POLICIES.map((value) => ({ value, label: t.dfs[value] }))}
                  />
                </div>
              </Show>
            </Show>
            <Button size="sm" variant="secondary" disabled={planning()} onClick={run}>
              {planning() ? `${t.planning}…` : plan() ? t.again : t.preview}
            </Button>
          </div>
        }
      />
      <Show when={plan()}>
        <Show when={banded()} fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.empty}</p>}>
          {(b) => (
            <>
              <BandPlanView plan={b()} nodeName={props.nodeName} />
              <Show when={props.admin && usable(b())}>
                <div class="flex flex-col gap-3 px-4 pb-4">
                  <Show
                    when={saved()?.band === props.band ? saved()?.revision : undefined}
                    fallback={
                      <div class="flex flex-wrap items-center gap-3">
                        <Button size="sm" disabled={using()} onClick={() => use(b())}>
                          {using() ? `${t.using}…` : t.use}
                        </Button>
                        <span class="text-fg-subtle text-xs">{t.useHint}</span>
                      </div>
                    }
                  >
                    {(revision) => <SavedRevision revision={revision()} note={t.savedNote} />}
                  </Show>
                </div>
              </Show>
            </>
          )}
        </Show>
      </Show>
    </Card>
  );
}

function BandPlanView(props: { plan: BandPlan; nodeName: (id: number) => string }) {
  const p = () => props.plan;
  const worth = () => p().improvement >= WORTH;
  const observed = () =>
    p().radios.length > 0 &&
    p().radios.every((r) => r.reasons.some((reason) => reason.kind === "observed"));
  const columns: Column<RadioPlan>[] = [
    {
      id: "radio",
      header: t.radio,
      cell: (r) => (
        <span class="flex flex-col">
          <a href={`/devices/${r.node}`} class="font-medium hover:text-accent hover:underline">
            {props.nodeName(r.node)}
          </a>
          <span class="text-fg-subtle text-xs">{r.radio}</span>
        </span>
      ),
    },
    {
      id: "now",
      header: t.now,
      cell: (r) => (
        <span class="whitespace-nowrap font-mono text-fg-muted text-xs tabular-nums">
          {r.current ? settingText(r.current) : t.down}
        </span>
      ),
    },
    {
      id: "planned",
      header: t.planned,
      cell: (r) => (
        <span
          class={[
            "whitespace-nowrap font-mono text-xs tabular-nums",
            r.changed ? "font-medium text-accent" : "text-fg-muted",
          ]}
        >
          {r.changed ? settingText(r.planned) : t.unchanged}
        </span>
      ),
    },
    {
      id: "cost",
      header: t.cost,
      align: "end",
      cell: (r) => (
        <span class="flex flex-col items-end">
          <span class="whitespace-nowrap font-mono text-xs">
            {`${r.before ? r.before.total.toFixed(2) : "—"} → ${r.after.total.toFixed(2)}`}
          </span>
          <span class="whitespace-nowrap text-fg-subtle text-xs">{termsText(r.after)}</span>
        </span>
      ),
    },
    {
      id: "why",
      header: t.why,
      class: "min-w-56",
      cell: (r) => (
        <ul class="flex flex-col gap-0.5 text-fg-muted text-xs">
          <For each={r.reasons}>{(reason) => <li>{reasonText(reason, props.nodeName)}</li>}</For>
        </ul>
      ),
    },
  ];
  return (
    <div class="flex flex-col gap-3 pb-4">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 text-sm">
        <span class="font-mono tabular-nums">
          {t.score(p().before.toFixed(2), p().after.toFixed(2))}
        </span>
        <Badge tone={worth() ? "ok" : "neutral"}>
          {p().improvement > 0 ? t.improvement(formatPercent(p().improvement * 100)) : t.noGain}
        </Badge>
        <span class="text-fg-subtle text-xs">{t.search[p().search]}</span>
        <Show when={!worth() && !observed()}>
          <span class="text-fg-subtle text-xs">{t.threshold}</span>
        </Show>
      </div>
      <Show when={observed()}>
        <p class="px-4 text-fg-muted text-xs">{t.allObserved}</p>
      </Show>
      <Table
        label={t.table}
        columns={columns}
        rows={p().radios}
        rowKey={(r) => `${r.node}/${r.radio}`}
        empty={<p class="py-6 text-center text-fg-subtle text-sm">{t.empty}</p>}
      />
      <Show when={p().skipped.length > 0}>
        <p class="px-4 text-fg-subtle text-xs">
          {t.skipped(
            p()
              .skipped.map((s) => `${props.nodeName(s.node)} ${s.radio}`)
              .join(", "),
          )}
        </p>
      </Show>
    </div>
  );
}
