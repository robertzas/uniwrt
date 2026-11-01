import { Search } from "lucide";
import { For, Show } from "solid-js";
import { strings } from "../../lib/strings";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import type { EntityGroup } from "./entities";

const m = strings.metrics;

/** The entities with series, grouped, with a filter; the chosen one is pressed. */
export function EntityList(props: {
  groups: readonly EntityGroup[];
  selected: string | undefined;
  filter: string;
  onFilter: (value: string) => void;
  onSelect: (entity: string) => void;
}) {
  return (
    <Card class="flex min-h-0 min-w-0 flex-col lg:sticky lg:top-4 lg:max-h-[calc(100dvh-7rem)]">
      <div class="border-edge border-b p-3">
        <label class="relative block">
          <span class="sr-only">{m.filter}</span>
          <Icon
            icon={Search}
            class="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2.5 size-4 text-fg-subtle"
          />
          <input
            type="search"
            value={props.filter}
            onInput={(event) => props.onFilter(event.currentTarget.value)}
            placeholder={m.filterPlaceholder}
            class="h-8 w-full rounded-control border border-edge bg-canvas pr-3 pl-8 text-sm outline-none placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
        </label>
      </div>
      <nav aria-label={m.entities} class="max-h-[50vh] min-h-0 overflow-y-auto p-2 lg:max-h-none">
        <Show
          when={props.groups.length > 0}
          fallback={<p class="px-2 py-6 text-center text-fg-subtle text-sm">{m.none}</p>}
        >
          <For each={props.groups}>
            {(group) => (
              <section class="mb-2 last:mb-0">
                <h3 class="px-2 pt-2 pb-1 font-medium text-fg-subtle text-xs uppercase tracking-wide">
                  {m.kind[group.kind]}
                </h3>
                <ul>
                  <For each={group.rows}>
                    {(row) => (
                      <li>
                        <button
                          type="button"
                          aria-pressed={props.selected === row.text ? "true" : "false"}
                          onClick={() => props.onSelect(row.text)}
                          title={row.text}
                          class={[
                            "flex w-full items-baseline gap-2 rounded-control py-1.5 pr-2 text-left text-sm transition-colors focus-visible:outline-2 focus-visible:outline-accent",
                            row.nested ? "pl-6" : "pl-2",
                            props.selected === row.text
                              ? "bg-nav-active text-nav-active-fg"
                              : "text-fg-muted hover:bg-raised hover:text-fg",
                          ]}
                        >
                          <span class="min-w-0 flex-1 truncate">
                            {row.nested && "name" in row.entity ? row.entity.name : row.label}
                          </span>
                          <span class="shrink-0 text-fg-subtle text-xs tabular-nums">
                            {row.info.metrics.length}
                          </span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </section>
            )}
          </For>
        </Show>
      </nav>
    </Card>
  );
}
