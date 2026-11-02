import { X, ZoomOut } from "lucide";
import { For, Show } from "solid-js";
import type { EventCategory } from "../../api/gen/EventCategory";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { CATEGORIES, categoryIcons, isLevel, LEVELS } from "../../lib/events";
import { formatDateTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { Segmented } from "../../ui/Segmented";
import {
  EMPTY_FILTER,
  type EventPreset,
  type EventRange,
  eventPresets,
  type Filter,
  isFiltered,
} from "./query";

const t = strings.events;

const SELECT =
  "h-8 cursor-pointer rounded-control border bg-surface pr-7 pl-2.5 text-sm focus-visible:outline-2 focus-visible:outline-accent";

export interface ToolbarProps {
  filter: Filter;
  range: EventRange;
  nodes: readonly NodeSummary[];
  clientName: (mac: string) => string;
  onFilter: (filter: Filter) => void;
  onRange: (range: EventRange) => void;
}

/** The page's filters: categories, the least level, a device, a client, and the range. */
export function Toolbar(props: ToolbarProps) {
  const set = (change: Partial<Filter>) => props.onFilter({ ...props.filter, ...change });
  const toggle = (category: EventCategory) => {
    const on = props.filter.categories.includes(category);
    set({
      categories: on
        ? props.filter.categories.filter((c) => c !== category)
        : CATEGORIES.filter((c) => c === category || props.filter.categories.includes(c)),
    });
  };
  const window = () => ("preset" in props.range ? undefined : props.range);
  return (
    <div class="flex flex-col gap-3">
      <fieldset class="flex flex-wrap gap-1.5">
        <legend class="sr-only">{t.categories}</legend>
        <For each={CATEGORIES}>
          {(category) => (
            <button
              type="button"
              aria-pressed={props.filter.categories.includes(category) ? "true" : "false"}
              onClick={() => toggle(category)}
              class={[
                "inline-flex h-8 items-center gap-1.5 rounded-full border px-3 text-sm transition-colors focus-visible:outline-2 focus-visible:outline-accent",
                props.filter.categories.includes(category)
                  ? "border-accent/50 bg-accent/10 text-fg"
                  : "border-edge text-fg-muted hover:bg-raised hover:text-fg",
              ]}
            >
              <Icon icon={categoryIcons[category]} class="size-3.5" />
              {t.category[category]}
            </button>
          )}
        </For>
      </fieldset>
      <div class="flex flex-wrap items-center gap-2">
        <label class="relative inline-flex h-8 items-center">
          <span class="sr-only">{t.level}</span>
          <select
            value={props.filter.min ?? ""}
            onChange={(event) => {
              const value = event.currentTarget.value;
              set({ min: isLevel(value) ? value : null });
            }}
            class={[
              SELECT,
              props.filter.min ? "border-accent/50 text-fg" : "border-edge text-fg-muted",
            ]}
          >
            <option value="">{t.anyLevel}</option>
            <For each={LEVELS.slice(0, 3)}>
              {(level) => (
                <option value={level}>
                  {level === "critical" ? t.levelName[level] : t.atLeast(t.levelName[level])}
                </option>
              )}
            </For>
          </select>
        </label>
        <label class="relative inline-flex h-8 items-center">
          <span class="sr-only">{t.device}</span>
          <select
            value={props.filter.nodes.length === 1 ? String(props.filter.nodes[0]) : ""}
            onChange={(event) => {
              const id = Number(event.currentTarget.value);
              set({ nodes: id > 0 ? [id] : [] });
            }}
            class={[
              SELECT,
              "max-w-[14rem]",
              props.filter.nodes.length > 0
                ? "border-accent/50 text-fg"
                : "border-edge text-fg-muted",
            ]}
          >
            <option value="">{t.anyDevice}</option>
            <For each={props.nodes}>
              {(node) => <option value={String(node.id)}>{node.name}</option>}
            </For>
          </select>
        </label>
        <For each={props.filter.clients}>
          {(mac) => (
            <span class="inline-flex h-8 items-center gap-1 rounded-full border border-accent/50 bg-accent/10 pr-1 pl-3 text-fg text-sm">
              {t.clientFilter(props.clientName(mac))}
              <button
                type="button"
                aria-label={t.removeClient(props.clientName(mac))}
                onClick={() => set({ clients: props.filter.clients.filter((c) => c !== mac) })}
                class="grid size-6 place-items-center rounded-full text-fg-subtle hover:bg-raised hover:text-fg"
              >
                <Icon icon={X} class="size-3.5" />
              </button>
            </span>
          )}
        </For>
        <Show when={isFiltered(props.filter)}>
          <Button size="sm" variant="ghost" icon={X} onClick={() => props.onFilter(EMPTY_FILTER)}>
            {t.clear}
          </Button>
        </Show>
        <div class="ml-auto flex flex-wrap items-center gap-2">
          <Show when={window()}>
            {(w) => (
              <span class="flex flex-wrap items-center gap-2 text-fg-subtle text-xs">
                <span class="tabular-nums">
                  {strings.chart.window(formatDateTime(w().from), formatDateTime(w().to))}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  icon={ZoomOut}
                  onClick={() => props.onRange({ preset: "24h" })}
                >
                  {strings.chart.resetZoom}
                </Button>
              </span>
            )}
          </Show>
          <Segmented<EventPreset>
            label={t.range}
            value={"preset" in props.range ? props.range.preset : ("" as EventPreset)}
            options={eventPresets.map((preset) => ({
              value: preset,
              label: t.ranges[preset][0],
              description: t.ranges[preset][1],
            }))}
            onChange={(preset) => props.onRange({ preset })}
          />
        </div>
      </div>
    </div>
  );
}
