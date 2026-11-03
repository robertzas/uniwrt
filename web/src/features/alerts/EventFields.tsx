import type { JSX } from "@solidjs/web";
import { createUniqueId, For, Show } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { CATEGORIES, isLevel, LEVELS } from "../../lib/events";
import { formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Checkbox } from "../../ui/Checkbox";
import { FormField, INPUT, SELECT } from "../../ui/FormField";
import { type Draft, type DraftErrors, KIND_GROUPS, WINDOW_CHOICES, withChoice } from "./rules";

const t = strings.alerts.editor;

/** `values` with `value` in or out. */
function toggled<T>(values: readonly T[], value: T, on: boolean): T[] {
  return on ? [...values.filter((v) => v !== value), value] : values.filter((v) => v !== value);
}

/** A titled box of checkboxes that scrolls past a few rows. */
function Group(props: {
  label: string;
  hint?: string | undefined;
  error?: string | undefined;
  children: JSX.Element;
  /** Grows with its content instead of scrolling. */
  full?: boolean;
}) {
  return (
    <fieldset class="flex min-w-0 flex-col gap-1.5">
      <legend class="mb-1.5 font-medium text-fg text-sm">{props.label}</legend>
      <div
        class={[
          "rounded-control border border-edge bg-canvas p-2.5",
          { "max-h-48 overflow-y-auto": !props.full },
        ]}
      >
        {props.children}
      </div>
      <p
        class={["text-xs", props.error ? "text-crit" : "text-fg-subtle"]}
        hidden={!props.error && !props.hint}
      >
        {props.error ?? props.hint}
      </p>
    </fieldset>
  );
}

/** An event rule's fields: which events, from where, how many within how long. */
export function EventFields(props: {
  draft: Draft;
  errors: DraftErrors;
  nodes: readonly NodeSummary[];
  patterns: readonly { id: string; meaning: string }[];
  onChange: (change: Partial<Draft>) => void;
}) {
  const id = createUniqueId();
  const d = () => props.draft;
  const logs = () => d().kinds.length === 0 || d().kinds.includes("log_pattern");

  return (
    <div class="flex flex-col gap-4">
      <Group full label={t.kinds} hint={t.kindsHint} error={props.errors.kinds}>
        <div class="gap-x-4 sm:columns-2">
          <For each={KIND_GROUPS}>
            {(group) => (
              <div class="mb-3 flex break-inside-avoid flex-col gap-1.5">
                <p class="font-medium text-fg-subtle text-xs uppercase tracking-wide">
                  {strings.alerts.kindGroups[group.group]}
                </p>
                <For each={group.kinds}>
                  {(kind) => (
                    <Checkbox
                      label={strings.alerts.kinds[kind]}
                      checked={d().kinds.includes(kind)}
                      onChange={(on) => props.onChange({ kinds: toggled(d().kinds, kind, on) })}
                    />
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Group>
      <Show when={logs() && props.patterns.length > 0}>
        <Group label={t.patterns} hint={t.patternsHint} error={props.errors.patterns}>
          <div class="flex flex-col gap-1.5">
            <For each={props.patterns}>
              {(pattern) => (
                <Checkbox
                  label={pattern.meaning}
                  hint={pattern.id}
                  checked={d().patterns.includes(pattern.id)}
                  onChange={(on) =>
                    props.onChange({ patterns: toggled(d().patterns, pattern.id, on) })
                  }
                />
              )}
            </For>
          </div>
        </Group>
      </Show>
      <fieldset class="flex min-w-0 flex-col gap-1.5">
        <legend class="mb-1.5 font-medium text-fg text-sm">{t.categories}</legend>
        <div class="flex flex-wrap gap-1.5">
          <For each={CATEGORIES}>
            {(category) => {
              const on = () => d().categories.includes(category);
              return (
                <button
                  type="button"
                  aria-pressed={on() ? "true" : "false"}
                  onClick={() =>
                    props.onChange({ categories: toggled(d().categories, category, !on()) })
                  }
                  class={[
                    "h-7 rounded-full border px-2.5 font-medium text-xs transition-colors focus-visible:outline-2 focus-visible:outline-accent",
                    on()
                      ? "border-accent/50 bg-accent/10 text-fg"
                      : "border-edge text-fg-subtle hover:text-fg",
                  ]}
                >
                  {strings.events.category[category]}
                </button>
              );
            }}
          </For>
        </div>
      </fieldset>
      <div class="grid gap-4 sm:grid-cols-2">
        <FormField label={t.min} for={`${id}-min`}>
          <select
            id={`${id}-min`}
            class={SELECT}
            value={d().min ?? ""}
            onChange={(event) => {
              const value = event.currentTarget.value;
              props.onChange({ min: isLevel(value) ? value : null });
            }}
          >
            <option value="">{t.anyLevel}</option>
            <For each={LEVELS}>
              {(level) => (
                <option value={level}>
                  {strings.events.atLeast(strings.events.levelName[level])}
                </option>
              )}
            </For>
          </select>
        </FormField>
        <div class="grid grid-cols-2 gap-3">
          <FormField label={t.count} for={`${id}-count`} error={props.errors.count}>
            <input
              id={`${id}-count`}
              inputmode="numeric"
              class={[INPUT, "tabular-nums", props.errors.count ? "border-crit" : "border-edge"]}
              value={d().count}
              onInput={(event) => props.onChange({ count: event.currentTarget.value })}
            />
          </FormField>
          <FormField label={t.window} for={`${id}-window`}>
            <select
              id={`${id}-window`}
              class={SELECT}
              value={String(d().windowS)}
              onChange={(event) => props.onChange({ windowS: Number(event.currentTarget.value) })}
            >
              <For each={withChoice(WINDOW_CHOICES, d().windowS)}>
                {(s) => <option value={String(s)}>{formatDuration(s)}</option>}
              </For>
            </select>
          </FormField>
        </div>
        <p class="text-fg-subtle text-xs sm:col-span-2">{t.countHint}</p>
      </div>
      <Show when={props.nodes.length > 0}>
        <Group label={t.nodes} hint={t.nodesHint} error={props.errors.nodes}>
          <div class="grid gap-1.5 sm:grid-cols-2">
            <For each={props.nodes}>
              {(node) => (
                <Checkbox
                  label={node.name}
                  checked={d().nodes.includes(node.id)}
                  onChange={(on) => props.onChange({ nodes: toggled(d().nodes, node.id, on) })}
                />
              )}
            </For>
          </div>
        </Group>
      </Show>
    </div>
  );
}
