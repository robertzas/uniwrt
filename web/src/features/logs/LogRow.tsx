import type { JSX } from "@solidjs/web";
import { ArrowUpRight, Copy, Filter, Lightbulb, ListTree } from "lucide";
import { createMemo, For, Show } from "solid-js";
import type { LogPattern } from "../../api/gen/LogPattern";
import type { LogRecord } from "../../api/gen/LogRecord";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { showToast } from "../../ui/Toast";
import { highlights, type Term } from "./match";
import { facilityName, LEVEL_FILL, levelOf } from "./severity";

const t = strings.logs;

/** Where an entry came from, as the page names it. */
export interface SourceLabel {
  name: string;
  /** The device's page, for a node's entries. */
  href?: string;
}

const time = new Intl.DateTimeFormat(undefined, {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  fractionalSecondDigits: 3,
  hourCycle: "h23",
});
const day = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const full = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "medium",
});

/** `09:30:00.125`, after the day when it is not today. */
export function logTime(ms: number, now = Date.now()): { day?: string; time: string } {
  const date = new Date(ms);
  const today = new Date(now).toDateString() === date.toDateString();
  return { ...(!today && { day: day.format(date) }), time: time.format(date) };
}

/** `program[pid]`, or just the program. */
export function programLabel(record: LogRecord): string | undefined {
  if (record.program === null) return undefined;
  return record.pid === null ? record.program : `${record.program}[${record.pid}]`;
}

/** The entry as one line of text, as a syslog file would hold it. */
export function logLine(record: LogRecord, source: string): string {
  const program = programLabel(record);
  return `${new Date(record.time).toISOString()} ${source} ${record.severity} ${program ? `${program}: ` : ""}${record.message}`;
}

/** The message with the search's words marked. */
export function Marked(props: { text: string; terms: readonly Term[] }) {
  const parts = createMemo(() => {
    const spans = highlights(props.text, props.terms);
    const out: { text: string; mark: boolean }[] = [];
    let at = 0;
    for (const [start, end] of spans) {
      if (start > at) out.push({ text: props.text.slice(at, start), mark: false });
      out.push({ text: props.text.slice(start, end), mark: true });
      at = end;
    }
    if (at < props.text.length) out.push({ text: props.text.slice(at), mark: false });
    return out;
  });
  return (
    <For each={parts()}>
      {(part) => (
        <Show when={part.mark} fallback={part.text}>
          <mark class="rounded-[2px] bg-accent/20 text-fg">{part.text}</mark>
        </Show>
      )}
    </For>
  );
}

export interface LogRowProps {
  record: LogRecord;
  source: SourceLabel;
  pattern: LogPattern | undefined;
  terms: readonly Term[];
  expanded: boolean;
  onToggle: () => void;
  onContext: () => void;
  onOnlySource: () => void;
  onOnlyProgram: () => void;
  /** Marks the entry a context view is about. */
  pivot?: boolean;
  /** In the context view: no context or filter actions. */
  compact?: boolean;
}

/** One entry: a line that expands into its details, annotation and actions. */
export function LogRow(props: LogRowProps) {
  const r = () => props.record;
  const when = () => logTime(r().time);
  const level = () => levelOf(r().severity);
  return (
    <div
      class={[
        "relative border-edge/60 border-b",
        props.expanded ? "bg-raised/60" : "hover:bg-raised/40",
        { "bg-accent/10": props.pivot },
      ]}
    >
      <span aria-hidden="true" class={["absolute inset-y-0 left-0 w-[3px]", LEVEL_FILL[level()]]} />
      <button
        type="button"
        aria-expanded={props.expanded ? "true" : "false"}
        onClick={() => props.onToggle()}
        class="flex w-full items-baseline gap-3 py-1.5 pr-3 pl-3.5 text-left text-[0.8125rem] focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <time
          datetime={new Date(r().time).toISOString()}
          class="w-[5.75rem] shrink-0 font-mono text-fg-subtle text-xs tabular-nums sm:w-[8.5rem]"
        >
          <Show when={when().day}>{(d) => <span class="mr-1.5 hidden sm:inline">{d()}</span>}</Show>
          {when().time}
        </time>
        <span class="w-12 shrink-0 font-mono font-medium text-[0.6875rem] text-fg-muted tracking-wide">
          {t.severityShort[r().severity]}
          <span class="sr-only"> ({t.severityName[r().severity]})</span>
        </span>
        <span class="hidden w-28 shrink-0 truncate text-fg-muted md:inline">
          {props.source.name}
        </span>
        <span class="hidden w-28 shrink-0 truncate font-mono text-fg-muted text-xs lg:inline">
          {programLabel(r()) ?? ""}
        </span>
        <span
          class={[
            "min-w-0 flex-1 font-mono text-fg",
            props.expanded ? "whitespace-pre-wrap [overflow-wrap:anywhere]" : "truncate",
          ]}
        >
          <Marked text={r().message} terms={props.terms} />
        </span>
        <Show when={props.pattern}>
          {(p) => (
            <span
              title={p().meaning}
              class="hidden max-w-[16rem] shrink-0 items-center gap-1 truncate rounded-full border border-accent/30 bg-accent/10 px-2 font-medium text-[0.6875rem] text-fg xl:inline-flex"
            >
              <Icon icon={Lightbulb} class="size-3 shrink-0 text-accent" />
              <span class="truncate">{p().meaning}</span>
            </span>
          )}
        </Show>
        <Show when={props.pattern}>
          <Icon icon={Lightbulb} class="size-3.5 shrink-0 self-center text-accent xl:hidden" />
          <span class="sr-only xl:hidden">{t.explained}</span>
        </Show>
      </button>
      <Show when={props.expanded}>
        <Details {...props} />
      </Show>
    </div>
  );
}

function Details(props: LogRowProps) {
  const r = () => props.record;
  const late = () => Math.abs(r().received - r().time) >= 1_000;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(logLine(r(), props.source.name));
      showToast({ title: t.copied, tone: "ok" });
    } catch {
      showToast({ title: t.copyFailed, tone: "crit" });
    }
  };
  return (
    <div class="flex flex-col gap-3 pr-4 pb-4 pl-3.5 sm:pl-[12.25rem]">
      <Show when={props.pattern}>
        {(p) => (
          <aside
            aria-label={t.annotation}
            class="flex gap-3 rounded-card border border-accent/30 bg-accent/5 p-3 text-sm"
          >
            <Icon icon={Lightbulb} class="mt-0.5 size-4 shrink-0 text-accent" />
            <div class="flex min-w-0 flex-col gap-1">
              <p class="font-medium text-fg">{p().meaning}</p>
              <Show when={p().hint}>{(hint) => <p class="text-fg-muted">{hint()}</p>}</Show>
              <Show when={p().link}>
                {(link) => (
                  <a
                    href={link()}
                    target="_blank"
                    rel="noopener noreferrer"
                    class="inline-flex w-fit items-center gap-0.5 font-medium text-accent text-xs hover:underline"
                  >
                    {t.learnMore}
                    <Icon icon={ArrowUpRight} class="size-3.5" />
                  </a>
                )}
              </Show>
            </div>
          </aside>
        )}
      </Show>
      <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-xs sm:grid-cols-[auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
        <Field label={t.details.logged}>
          <span class="font-mono tabular-nums">{full.format(new Date(r().time))}</span>
        </Field>
        <Show when={late()}>
          <Field label={t.details.received}>
            <span class="font-mono tabular-nums">{full.format(new Date(r().received))}</span>
          </Field>
        </Show>
        <Field label={t.details.source}>
          <Show when={props.source.href} fallback={props.source.name}>
            {(href) => (
              <a href={href()} class="font-medium text-accent hover:underline">
                {props.source.name}
              </a>
            )}
          </Show>
        </Field>
        <Show when={programLabel(r())}>
          {(program) => (
            <Field label={t.details.program}>
              <span class="font-mono">{program()}</span>
            </Field>
          )}
        </Show>
        <Field label={t.details.severity}>{t.severityName[r().severity]}</Field>
        <Field label={t.details.facility}>
          <span class="font-mono">{facilityName(r().facility)}</span>
        </Field>
        <Field label={t.details.via}>{t.via[r().via]}</Field>
        <Show when={r().seq !== null}>
          <Field label={t.details.seq}>
            <span class="font-mono tabular-nums">{r().seq}</span>
          </Field>
        </Show>
      </dl>
      <div class="flex flex-wrap gap-2">
        <Show when={!props.compact}>
          <Button size="sm" icon={ListTree} onClick={() => props.onContext()}>
            {t.context}
          </Button>
          <Button size="sm" variant="ghost" icon={Filter} onClick={() => props.onOnlySource()}>
            {t.onlySource}
          </Button>
          <Show when={r().program !== null}>
            <Button size="sm" variant="ghost" icon={Filter} onClick={() => props.onOnlyProgram()}>
              {t.onlyProgram}
            </Button>
          </Show>
        </Show>
        <Button size="sm" variant="ghost" icon={Copy} onClick={() => void copy()}>
          {t.copy}
        </Button>
      </div>
    </div>
  );
}

function Field(props: { label: string; children: JSX.Element }) {
  return (
    <>
      <dt class="text-fg-subtle">{props.label}</dt>
      <dd class="min-w-0 truncate text-fg">{props.children}</dd>
    </>
  );
}
