import { createUniqueId, For, Show } from "solid-js";
import type { EventLevel } from "../../../api/gen/EventLevel";
import type { NodeSummary } from "../../../api/gen/NodeSummary";
import { LEVELS } from "../../../lib/events";
import { strings } from "../../../lib/strings";
import { Checkbox } from "../../../ui/Checkbox";
import { FormField, SELECT } from "../../../ui/FormField";
import { Segmented } from "../../../ui/Segmented";
import { type Draft, type DraftErrors, REMIND_CHOICES } from "./draft";
import { remindName } from "./words";

const t = strings.settings.notifications.filter;

/** Which alerts a notifier hears of: the least level, the devices, resolutions, reminders. */
export function FilterFields(props: {
  draft: Draft;
  errors: DraftErrors;
  nodes: readonly NodeSummary[];
  onChange: (change: Partial<Draft>) => void;
}) {
  const id = createUniqueId();
  const d = () => props.draft;
  const reminders = () =>
    REMIND_CHOICES.includes(d().remind) ? REMIND_CHOICES : [...REMIND_CHOICES, d().remind];
  const toggle = (node: number, on: boolean) =>
    props.onChange({
      nodes: on
        ? [...d().nodes.filter((n) => n !== node), node]
        : d().nodes.filter((n) => n !== node),
    });

  return (
    <fieldset class="flex flex-col gap-4 rounded-card border border-edge p-4">
      <legend class="px-1 font-medium text-fg text-sm">{t.title}</legend>
      <div class="flex flex-col gap-1.5">
        <span class="font-medium text-fg text-sm">{t.min}</span>
        <Segmented
          label={t.min}
          value={d().min}
          options={LEVELS.map((level) => ({
            value: level,
            label: strings.events.levelName[level],
          }))}
          onChange={(min: EventLevel) => props.onChange({ min })}
          class="self-start"
        />
        <p class="text-fg-subtle text-xs">{t.minHint[d().min]}</p>
      </div>
      <Show when={props.nodes.length > 0}>
        <div class="flex flex-col gap-1.5">
          <span class="font-medium text-fg text-sm">{t.devices}</span>
          <Segmented
            label={t.devices}
            value={d().someNodes ? "some" : "every"}
            options={[
              { value: "every", label: t.everyDevice },
              { value: "some", label: t.someDevices },
            ]}
            onChange={(value) => props.onChange({ someNodes: value === "some" })}
            class="self-start"
          />
          <Show when={d().someNodes}>
            <div class="mt-1 grid gap-x-4 gap-y-2 sm:grid-cols-2">
              <For each={props.nodes}>
                {(node) => (
                  <Checkbox
                    label={node.name}
                    checked={d().nodes.includes(node.id)}
                    onChange={(on) => toggle(node.id, on)}
                  />
                )}
              </For>
            </div>
          </Show>
          <p class={["text-xs", props.errors.nodes ? "text-crit" : "text-fg-subtle"]}>
            {props.errors.nodes ?? (d().someNodes ? t.someHint : t.everyHint)}
          </p>
        </div>
      </Show>
      <div class="flex flex-wrap items-end gap-x-6 gap-y-4">
        <Checkbox
          label={t.resolved}
          hint={t.resolvedHint}
          checked={d().resolved}
          onChange={(resolved) => props.onChange({ resolved })}
          class="max-w-72"
        />
        <FormField label={t.remind} for={`${id}-remind`} hint={t.remindHint} class="w-full sm:w-56">
          <select
            id={`${id}-remind`}
            class={SELECT}
            value={String(d().remind)}
            onChange={(event) => props.onChange({ remind: Number(event.currentTarget.value) })}
          >
            <For each={reminders()}>
              {(s) => <option value={String(s)}>{remindName(s)}</option>}
            </For>
          </select>
        </FormField>
      </div>
    </fieldset>
  );
}
