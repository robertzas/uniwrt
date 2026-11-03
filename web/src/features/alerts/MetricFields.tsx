import { createUniqueId, For, Show } from "solid-js";
import type { Comparison } from "../../api/gen/Comparison";
import type { Metric } from "../../api/gen/Metric";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { formatDuration } from "../../lib/format";
import { formatMetric, METRIC_UNITS, metricLabel } from "../../lib/metrics";
import { strings } from "../../lib/strings";
import { Segmented } from "../../ui/Segmented";
import {
  type Draft,
  type DraftErrors,
  FOR_CHOICES,
  MEASURES,
  TARGETS,
  type TargetKind,
  withChoice,
} from "./rules";
import { FormField, INPUT, SELECT } from "./shared";

const t = strings.alerts.editor;

/** A metric rule's fields: what to watch, the metric, the threshold and how long. */
export function MetricFields(props: {
  draft: Draft;
  errors: DraftErrors;
  nodes: readonly NodeSummary[];
  onChange: (change: Partial<Draft>) => void;
}) {
  const id = createUniqueId();
  const d = () => props.draft;
  const target = () => d().target;
  const metrics = () => MEASURES[target().kind];
  const unit = () => t.units[METRIC_UNITS[d().metric]];
  const threshold = () => Number(d().threshold);
  const setTarget = (change: Partial<Draft["target"]>) => {
    const next = { ...target(), ...change };
    const kept = MEASURES[next.kind].includes(d().metric);
    props.onChange({ target: next, ...(!kept && { metric: MEASURES[next.kind][0] ?? "cpu" }) });
  };

  return (
    <div class="grid gap-4 sm:grid-cols-2">
      <FormField label={t.target} for={`${id}-kind`}>
        <select
          id={`${id}-kind`}
          class={SELECT}
          value={target().kind}
          onChange={(event) =>
            setTarget({ kind: event.currentTarget.value as TargetKind, part: "" })
          }
        >
          <For each={TARGETS}>
            {(kind) => <option value={kind}>{strings.alerts.target.kind[kind]}</option>}
          </For>
        </select>
      </FormField>
      <Show when={["node", "device", "radio"].includes(target().kind)}>
        <FormField label={t.which} for={`${id}-node`}>
          <select
            id={`${id}-node`}
            class={SELECT}
            value={target().node === null ? "" : String(target().node)}
            onChange={(event) => {
              const value = Number(event.currentTarget.value);
              setTarget({ node: value > 0 ? value : null });
            }}
          >
            <option value="">{strings.alerts.target.anyDevice}</option>
            <For each={props.nodes}>
              {(node) => <option value={String(node.id)}>{node.name}</option>}
            </For>
          </select>
        </FormField>
      </Show>
      <Show when={target().kind === "device" || target().kind === "radio"}>
        <FormField
          label={t.partName}
          for={`${id}-part`}
          error={props.errors.part}
          hint={strings.alerts.target.anyName}
        >
          <input
            id={`${id}-part`}
            class={[INPUT, props.errors.part ? "border-crit" : "border-edge"]}
            value={target().part}
            placeholder={t.partPlaceholder[target().kind === "device" ? "device" : "radio"]}
            autocapitalize="none"
            spellcheck={false}
            onInput={(event) => setTarget({ part: event.currentTarget.value })}
          />
        </FormField>
      </Show>
      <Show when={target().kind === "client"}>
        <FormField label={t.client} for={`${id}-client`} error={props.errors.part}>
          <input
            id={`${id}-client`}
            class={[INPUT, "font-mono", props.errors.part ? "border-crit" : "border-edge"]}
            value={target().part}
            placeholder={t.clientPlaceholder}
            autocapitalize="none"
            spellcheck={false}
            onInput={(event) => setTarget({ part: event.currentTarget.value })}
          />
        </FormField>
      </Show>
      <FormField label={t.metric} for={`${id}-metric`} class="sm:col-start-1">
        <select
          id={`${id}-metric`}
          class={SELECT}
          value={d().metric}
          onChange={(event) => props.onChange({ metric: event.currentTarget.value as Metric })}
        >
          <For each={metrics()}>
            {(metric) => <option value={metric}>{metricLabel(metric)}</option>}
          </For>
        </select>
      </FormField>
      <div class="flex flex-col gap-1.5">
        <span class="font-medium text-fg text-sm">{t.op}</span>
        <Segmented
          label={t.op}
          value={d().op}
          options={(["above", "below"] as const).map((op) => ({ value: op, label: t.ops[op] }))}
          onChange={(op: Comparison) => props.onChange({ op })}
          class="self-start"
        />
      </div>
      <FormField
        label={t.threshold}
        for={`${id}-threshold`}
        error={props.errors.threshold}
        hint={
          Number.isFinite(threshold()) && d().threshold.trim() !== ""
            ? t.thresholdHint(formatMetric(d().metric, threshold()))
            : undefined
        }
      >
        <div class="flex items-center gap-2">
          <input
            id={`${id}-threshold`}
            inputmode="decimal"
            class={[INPUT, "tabular-nums", props.errors.threshold ? "border-crit" : "border-edge"]}
            value={d().threshold}
            onInput={(event) => props.onChange({ threshold: event.currentTarget.value })}
          />
          <Show when={unit()}>
            <span class="shrink-0 text-fg-subtle text-sm">{unit()}</span>
          </Show>
        </div>
      </FormField>
      <FormField
        label={t.clear}
        for={`${id}-clear`}
        error={props.errors.clear}
        hint={t.clearHint(d().threshold.trim() === "" ? "…" : d().threshold)}
      >
        <div class="flex items-center gap-2">
          <input
            id={`${id}-clear`}
            inputmode="decimal"
            class={[INPUT, "tabular-nums", props.errors.clear ? "border-crit" : "border-edge"]}
            value={d().clear}
            onInput={(event) => props.onChange({ clear: event.currentTarget.value })}
          />
          <Show when={unit()}>
            <span class="shrink-0 text-fg-subtle text-sm">{unit()}</span>
          </Show>
        </div>
      </FormField>
      <FormField label={t.for} for={`${id}-for`}>
        <select
          id={`${id}-for`}
          class={SELECT}
          value={String(d().forS)}
          onChange={(event) => props.onChange({ forS: Number(event.currentTarget.value) })}
        >
          <For each={withChoice(FOR_CHOICES, d().forS)}>
            {(s) => (
              <option value={String(s)}>{s === 0 ? t.immediately : formatDuration(s)}</option>
            )}
          </For>
        </select>
      </FormField>
    </div>
  );
}
