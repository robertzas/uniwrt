import { ChevronRight, Lock } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { RawSection } from "../../api/gen/RawSection";
import type { RawValue } from "../../api/gen/RawValue";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";

const t = strings.changes.node.raw;

/**
 * One option as `uci` would write it; a list item by item, a sealed secret as a lock. `where`:
 * an option the section is found by.
 */
function Option(props: { name: string; value: RawValue; where?: boolean }) {
  const items = () => {
    const value = props.value;
    return Array.isArray(value) ? value : [value];
  };
  return (
    <For each={items()}>
      {(item) => (
        <li class="py-0.5 pr-3 pl-8">
          <span class="text-fg-subtle">
            {props.where ? t.where : Array.isArray(props.value) ? "list" : "option"}
          </span>{" "}
          <span class="text-fg-muted">{props.name}</span>{" "}
          <Show
            when={typeof item === "string"}
            fallback={
              <span
                class="inline-flex items-center gap-1 rounded border border-edge bg-raised px-1 font-sans text-fg-muted"
                title={t.secretHint}
              >
                <Icon icon={Lock} class="size-3" />
                {t.secret}
              </span>
            }
          >
            <span class="break-all text-fg">'{item as string}'</span>
          </Show>
        </li>
      )}
    </For>
  );
}

/** What a raw section does to its section. */
function kindOf(section: RawSection): string {
  if (section.remove) return t.removed;
  return section.merge ? t.merged : t.whole;
}

/**
 * A node's raw overrides (lifecycle.md, Brownfield import; D-143): each section in the site's
 * order, kept whole, merged into its section (options set and deleted) or removing it, found
 * by name or by the options it matches; closed until opened.
 */
export function RawOverrides(props: { raw: readonly RawSection[] }) {
  const [open, setOpen] = createSignal(false);
  return (
    <div class="overflow-hidden rounded-control border border-edge">
      <button
        type="button"
        aria-expanded={open() ? "true" : "false"}
        onClick={() => setOpen((now) => !now)}
        class="flex w-full items-center gap-2 bg-raised/50 px-3 py-1.5 text-left hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <Icon
          icon={ChevronRight}
          class={["size-3.5 shrink-0 text-fg-subtle transition-transform", { "rotate-90": open() }]}
        />
        <span class="font-medium text-fg text-xs">{t.toggle}</span>
        <span class="ml-auto text-fg-subtle text-xs tabular-nums">{t.count(props.raw.length)}</span>
      </button>
      <Show when={open()}>
        <p class="border-edge/60 border-t px-3 py-2 text-fg-muted text-xs">{t.hint}</p>
        <ol class="divide-y divide-edge/50 border-edge/60 border-t font-mono text-xs leading-5">
          <For each={props.raw}>
            {(section) => (
              <li class="py-1">
                <div class="flex min-w-0 flex-wrap items-baseline gap-x-2 px-3 py-0.5">
                  <span class="min-w-0">
                    <span class="text-fg-subtle">/etc/config/{section.package}</span>{" "}
                    <span class="text-fg-subtle">config</span>{" "}
                    <span class="text-fg">{section.type}</span>
                    <Show when={section.name}>
                      {(name) => <span class="text-fg"> '{name()}'</span>}
                    </Show>
                  </span>
                  <span class="font-sans text-[0.6875rem] text-fg-muted">{kindOf(section)}</span>
                </div>
                <ul>
                  <For each={Object.entries(section.matching)}>
                    {([name, value]) => <Option name={name} value={value} where />}
                  </For>
                  <For each={Object.entries(section.options)}>
                    {([name, value]) => <Option name={name} value={value} />}
                  </For>
                  <For each={section.unset}>
                    {(name) => (
                      <li class="py-0.5 pr-3 pl-8">
                        <span class="text-fg-subtle">{t.deleted}</span>{" "}
                        <span class="text-crit line-through">{name}</span>
                      </li>
                    )}
                  </For>
                </ul>
              </li>
            )}
          </For>
        </ol>
      </Show>
    </div>
  );
}
