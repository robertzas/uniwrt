import { Lightbulb, Search, Server, SquareTerminal, X, ZoomOut } from "lucide";
import { createEffect, createSignal, For, onSettled, Show } from "solid-js";
import type { Severity } from "../../api/gen/Severity";
import { formatDateTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { Segmented } from "../../ui/Segmented";
import { Picker, type PickerGroup } from "./Picker";
import { type Filter, isFiltered, type LogPreset, type LogRange, logPresets } from "./query";
import { SavedSearches } from "./SavedSearches";
import { isSeverity, SEVERITIES } from "./severity";

const t = strings.logs;
/** How long typing pauses before the search runs. */
const DEBOUNCE_MS = 300;

export interface ToolbarProps {
  filter: Filter;
  range: LogRange;
  sourceGroups: readonly PickerGroup[];
  programs: readonly string[];
  onFilter: (filter: Filter) => void;
  onRange: (range: LogRange) => void;
  /** A picker opened: the page reads the sources again. */
  onSources: () => void;
}

/** The page's filters: search, severity, sources, programs, explained only, saved, range. */
export function Toolbar(props: ToolbarProps) {
  const set = (change: Partial<Filter>) => props.onFilter({ ...props.filter, ...change });
  const sources = () => [
    ...props.filter.nodes.map((id) => `node:${id}`),
    ...props.filter.hosts.map((host) => `host:${host}`),
  ];
  const onSources = (values: string[]) => {
    const nodes = values.filter((v) => v.startsWith("node:")).map((v) => Number(v.slice(5)));
    const hosts = values.filter((v) => v.startsWith("host:")).map((v) => v.slice(5));
    set({ nodes, hosts });
  };
  const programGroups = (): PickerGroup[] => {
    const all = [...new Set([...props.programs, ...props.filter.programs])].sort();
    return [{ label: t.programs, options: all.map((p) => ({ value: p, label: p })) }];
  };
  const window = () => ("preset" in props.range ? undefined : props.range);
  return (
    <div class="flex flex-col gap-3">
      <div class="flex flex-wrap items-center gap-2">
        <SearchBox text={props.filter.text} onSearch={(text) => set({ text })} />
        <label class="relative inline-flex h-8 items-center">
          <span class="sr-only">{t.severity}</span>
          <select
            value={props.filter.min ?? ""}
            onChange={(event) => {
              const value = event.currentTarget.value;
              set({ min: isSeverity(value) ? (value as Severity) : null });
            }}
            class={[
              "h-8 cursor-pointer rounded-control border bg-surface pr-7 pl-2.5 text-sm focus-visible:outline-2 focus-visible:outline-accent",
              props.filter.min ? "border-accent/50 text-fg" : "border-edge text-fg-muted",
            ]}
          >
            <option value="">{t.anySeverity}</option>
            <For each={SEVERITIES.slice(0, 7)}>
              {(severity) => (
                <option value={severity}>
                  {severity === "emergency"
                    ? t.severityName[severity]
                    : t.atLeast(t.severityName[severity])}
                </option>
              )}
            </For>
          </select>
        </label>
        <Picker
          label={t.sources}
          all={t.anySource}
          icon={Server}
          groups={props.sourceGroups}
          selected={sources()}
          onChange={onSources}
          onOpen={props.onSources}
        />
        <Picker
          label={t.programs}
          all={t.anyProgram}
          icon={SquareTerminal}
          groups={programGroups()}
          selected={props.filter.programs}
          onChange={(programs) => set({ programs })}
          onOpen={props.onSources}
        />
        <button
          type="button"
          aria-pressed={props.filter.annotated ? "true" : "false"}
          title={t.annotatedHint}
          onClick={() => set({ annotated: !props.filter.annotated })}
          class={[
            "inline-flex h-8 items-center gap-1.5 rounded-control border px-2.5 text-sm transition-colors focus-visible:outline-2 focus-visible:outline-accent",
            props.filter.annotated
              ? "border-accent/50 bg-accent/10 text-fg"
              : "border-edge text-fg-muted hover:bg-raised hover:text-fg",
          ]}
        >
          <Icon icon={Lightbulb} class="size-4 text-accent" />
          {t.annotated}
        </button>
        <Show when={isFiltered(props.filter)}>
          <Button
            size="sm"
            variant="ghost"
            icon={X}
            onClick={() =>
              props.onFilter({
                nodes: [],
                hosts: [],
                programs: [],
                min: null,
                text: null,
                annotated: false,
              })
            }
          >
            {t.clear}
          </Button>
        </Show>
        <div class="ml-auto">
          <SavedSearches filter={props.filter} onApply={props.onFilter} />
        </div>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <Segmented<LogPreset>
          label={t.range}
          value={"preset" in props.range ? props.range.preset : ("" as LogPreset)}
          options={logPresets.map((preset) => ({
            value: preset,
            label: t.ranges[preset][0],
            description: t.ranges[preset][1],
          }))}
          onChange={(preset) => props.onRange({ preset })}
        />
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
                onClick={() => props.onRange({ preset: "1h" })}
              >
                {strings.chart.resetZoom}
              </Button>
            </span>
          )}
        </Show>
      </div>
    </div>
  );
}

/**
 * The full-text search: it runs once typing pauses, or at Enter, and follows the URL when a
 * saved search or a link changes it. `/` focuses it from anywhere on the page.
 */
function SearchBox(props: { text: string | null; onSearch: (text: string | null) => void }) {
  let input: HTMLInputElement | undefined;
  const [draft, setDraft] = createSignal("");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let sent: string | null = null;
  createEffect(
    () => props.text,
    (text) => {
      if (text !== sent) {
        sent = text;
        setDraft(text ?? "");
      }
    },
  );
  const commit = (value: string) => {
    clearTimeout(timer);
    const text = value.trim() === "" ? null : value;
    if (text === sent) return;
    sent = text;
    props.onSearch(text);
  };
  onSettled(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing =
        target?.isContentEditable ||
        ["INPUT", "TEXTAREA", "SELECT"].includes(target?.tagName ?? "");
      if (event.key === "/" && !typing && !event.metaKey && !event.ctrlKey) {
        event.preventDefault();
        input?.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("keydown", onKey);
    };
  });
  return (
    <label class="relative flex h-8 min-w-[14rem] flex-1 items-center sm:max-w-md">
      <span class="sr-only">{t.search}</span>
      <Icon icon={Search} class="pointer-events-none absolute left-2.5 size-4 text-fg-subtle" />
      <input
        ref={input}
        type="search"
        value={draft()}
        placeholder={t.searchPlaceholder}
        title={t.searchHelp}
        spellcheck={false}
        autocomplete="off"
        onInput={(event) => {
          const value = event.currentTarget.value;
          setDraft(value);
          clearTimeout(timer);
          timer = setTimeout(() => commit(value), DEBOUNCE_MS);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit(event.currentTarget.value);
          if (event.key === "Escape" && draft() !== "") {
            setDraft("");
            commit("");
          }
        }}
        class="h-8 w-full rounded-control border border-edge bg-surface pr-8 pl-8 font-mono text-sm placeholder:font-sans placeholder:text-fg-subtle focus-visible:outline-2 focus-visible:outline-accent"
      />
      <span
        aria-hidden="true"
        class="pointer-events-none absolute right-2 hidden rounded border border-edge px-1 font-mono text-[0.6875rem] text-fg-subtle sm:block"
      >
        /
      </span>
    </label>
  );
}
