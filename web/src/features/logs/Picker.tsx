import * as Popover from "@kobalte/core/popover";
import { ChevronDown, Search } from "lucide";
import { createMemo, createSignal, For, Show } from "solid-js";
import { strings } from "../../lib/strings";
import { Checkbox } from "../../ui/Checkbox";
import { Icon, type IconNode } from "../../ui/Icon";

const t = strings.logs.picker;

export interface PickerOption {
  value: string;
  label: string;
  /** A second line, such as a device's address. */
  hint?: string;
}

export interface PickerGroup {
  label: string;
  options: readonly PickerOption[];
}

/** Options whose label, value or hint holds every word of `filter`. */
export function filterGroups(groups: readonly PickerGroup[], filter: string): PickerGroup[] {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  return groups
    .map((group) => ({
      label: group.label,
      options: group.options.filter((option) => {
        const text = `${option.label} ${option.value} ${option.hint ?? ""}`.toLowerCase();
        return words.every((word) => text.includes(word));
      }),
    }))
    .filter((group) => group.options.length > 0);
}

/**
 * A button that opens a list of checkboxes, filterable, in groups: the logs' sources and
 * programs. Choosing nothing means everything (`all` names it on the button).
 */
export function Picker(props: {
  label: string;
  all: string;
  icon: IconNode;
  groups: readonly PickerGroup[];
  selected: readonly string[];
  onChange: (selected: string[]) => void;
  /** Opening the list: the page reads the sources again. */
  onOpen?: () => void;
}) {
  const [filter, setFilter] = createSignal("");
  const shown = createMemo(() => filterGroups(props.groups, filter()));
  const names = createMemo(() => {
    const byValue = new Map<string, string>();
    for (const group of props.groups)
      for (const option of group.options) byValue.set(option.value, option.label);
    return byValue;
  });
  const summary = () => {
    const [first] = props.selected;
    if (first === undefined) return props.all;
    if (props.selected.length === 1) return names().get(first) ?? first;
    return t.count(props.selected.length);
  };
  const toggle = (value: string, on: boolean) => {
    const rest = props.selected.filter((v) => v !== value);
    props.onChange(on ? [...rest, value] : rest);
  };
  return (
    <Popover.Root
      placement="bottom-start"
      gutter={6}
      onOpenChange={(open) => {
        if (open) props.onOpen?.();
        else setFilter("");
      }}
    >
      <Popover.Trigger
        aria-label={`${props.label}: ${summary()}`}
        class={[
          "inline-flex h-8 max-w-[14rem] items-center gap-2 rounded-control border px-2.5 text-sm transition-colors hover:bg-raised data-expanded:bg-raised focus-visible:outline-2 focus-visible:outline-accent",
          props.selected.length > 0 ? "border-accent/50 text-fg" : "border-edge text-fg-muted",
        ]}
      >
        <Icon icon={props.icon} class="size-4 shrink-0 text-fg-subtle" />
        <span class="truncate">{summary()}</span>
        <Icon icon={ChevronDown} class="size-3.5 shrink-0 text-fg-subtle" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 flex max-h-[min(28rem,70vh)] w-72 flex-col rounded-card border border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
          <Popover.Title class="sr-only">{props.label}</Popover.Title>
          <div class="flex items-center gap-2 border-edge border-b px-3">
            <Icon icon={Search} class="size-4 shrink-0 text-fg-subtle" />
            <input
              type="search"
              aria-label={`${t.filter} ${props.label.toLowerCase()}`}
              placeholder={t.filter}
              value={filter()}
              onInput={(event) => setFilter(event.currentTarget.value)}
              class="h-9 w-full bg-transparent text-sm outline-none placeholder:text-fg-subtle"
            />
          </div>
          <div class="min-h-0 flex-1 overflow-auto p-2">
            <Show
              when={shown().length > 0}
              fallback={
                <p class="px-2 py-6 text-center text-fg-subtle text-sm">
                  {props.groups.some((g) => g.options.length > 0) ? t.none : t.empty}
                </p>
              }
            >
              <For each={shown()}>
                {(group) => (
                  <fieldset class="mb-2 last:mb-0">
                    <legend class="px-2 pb-1 font-medium text-[0.6875rem] text-fg-subtle uppercase tracking-wide">
                      {group.label}
                    </legend>
                    <For each={group.options}>
                      {(option) => (
                        <Checkbox
                          class="rounded-control px-2 py-1.5 hover:bg-raised"
                          label={option.label}
                          hint={option.hint}
                          checked={props.selected.includes(option.value)}
                          onChange={(on) => toggle(option.value, on)}
                        />
                      )}
                    </For>
                  </fieldset>
                )}
              </For>
            </Show>
          </div>
          <Show when={props.selected.length > 0}>
            <div class="border-edge border-t p-2">
              <button
                type="button"
                onClick={() => props.onChange([])}
                class="h-7 w-full rounded-control font-medium text-fg-muted text-xs hover:bg-raised hover:text-fg"
              >
                {t.clear}
              </button>
            </div>
          </Show>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
