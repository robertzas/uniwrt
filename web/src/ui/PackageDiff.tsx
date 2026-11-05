import type { JSX } from "@solidjs/web";
import { ChevronRight, Lock } from "lucide";
import { createSignal, For, Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "./Icon";
import {
  diffCounts,
  MASKED,
  type OptionLine,
  optionLines,
  renamedFrom,
  type SectionChange,
  sectionChange,
  sectionLabel,
  type UciDiff,
  type UciSectionDiff,
  type UciSectionId,
} from "./uciDiff";

const t = strings.changes.diff;

/** A package opens by itself up to this many changed sections. */
const OPEN_UP_TO = 8;

const OPS: Record<OptionLine["op"], { sign: string; row: string; tone: string }> = {
  "+": { sign: "+", row: "bg-ok/8", tone: "text-ok" },
  "-": { sign: "−", row: "bg-crit/8", tone: "text-crit" },
  "~": { sign: "~", row: "bg-warn/8", tone: "text-warn" },
};

const OP_WORDS = { "+": "added", "-": "removed", "~": "changed" } as const;

const SECTION_OPS: Record<SectionChange, OptionLine["op"]> = {
  added: "+",
  removed: "-",
  changed: "~",
  moved: "~",
};

/** A value as `uci` quotes it; a masked secret as a lock, never its text. */
function Quoted(props: { value: string }) {
  return (
    <Show
      when={props.value !== MASKED}
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
      <span class="break-all text-fg">'{props.value}'</span>
    </Show>
  );
}

function Line(props: { line: OptionLine }) {
  const op = () => OPS[props.line.op];
  return (
    <li class={["flex min-w-0 items-baseline gap-2 py-0.5 pr-3 pl-8", op().row]}>
      <span class={["w-3 shrink-0 select-none font-semibold", op().tone]} aria-hidden="true">
        {op().sign}
      </span>
      <span class="sr-only">{t[OP_WORDS[props.line.op]]}</span>
      <span class="min-w-0 flex-1">
        <span class="text-fg-subtle">{props.line.keyword}</span>{" "}
        <span class="text-fg-muted">{props.line.name}</span>{" "}
        <Show when={props.line.before !== undefined}>
          <span class="text-fg-subtle line-through decoration-fg-subtle/60">
            <Quoted value={props.line.before ?? ""} />
          </span>
          <span class="px-1 text-fg-subtle" aria-hidden="true">
            →
          </span>
        </Show>
        <Quoted value={props.line.value} />
      </span>
    </li>
  );
}

function Section(props: { section: UciSectionDiff }) {
  const change = () => sectionChange(props.section);
  // A diff has a section before or after, or both.
  const id = () => (props.section.after ?? props.section.before) as UciSectionId;
  const op = () => OPS[SECTION_OPS[change()]];
  return (
    <li class="py-1">
      <div class="flex min-w-0 flex-wrap items-baseline gap-x-2 px-3 py-0.5">
        <span class={["w-3 shrink-0 select-none font-semibold", op().tone]} aria-hidden="true">
          {op().sign}
        </span>
        <span class="min-w-0">
          <span class="text-fg-subtle">config</span> <span class="text-fg">{id().type}</span>{" "}
          <span class="text-fg">{sectionLabel(id())}</span>
        </span>
        <Show when={renamedFrom(props.section)}>
          {(from) => <span class="text-fg-subtle">({t.renamed(from())})</span>}
        </Show>
        <span class="font-sans text-[0.6875rem] text-fg-muted">
          {t[change()]}
          <Show when={props.section.moved && change() !== "moved"}> · {t.moved}</Show>
        </span>
      </div>
      <Show when={change() === "moved"}>
        <p class="pl-8 font-sans text-fg-subtle">{t.movedHint}</p>
      </Show>
      <ul>
        <For each={props.section.options.flatMap(optionLines)}>
          {(line) => <Line line={line} />}
        </For>
      </ul>
    </li>
  );
}

/**
 * One package's semantic diff, as the node holds it → as rendered, opening and closing; it
 * starts open when small, unless `open` says otherwise.
 */
export function PackageDiff(props: { diff: UciDiff; open?: boolean; badge?: JSX.Element }) {
  const [open, setOpen] = createSignal(props.open ?? props.diff.sections.length <= OPEN_UP_TO);
  const c = () => diffCounts(props.diff);
  return (
    <div class="overflow-hidden rounded-control border border-edge">
      <button
        type="button"
        aria-expanded={open() ? "true" : "false"}
        aria-label={t.toggle(props.diff.package)}
        onClick={() => setOpen((now) => !now)}
        class="flex w-full items-center gap-2 bg-raised/50 px-3 py-1.5 text-left hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <Icon
          icon={ChevronRight}
          class={["size-3.5 shrink-0 text-fg-subtle transition-transform", { "rotate-90": open() }]}
        />
        <span class="font-medium font-mono text-fg text-xs">/etc/config/{props.diff.package}</span>
        {props.badge}
        <span class="ml-auto text-fg-subtle text-xs tabular-nums">
          {t.count(c().added, c().removed, c().changed)}
        </span>
      </button>
      <Show when={open()}>
        <ol class="divide-y divide-edge/50 border-edge/60 border-t font-mono text-xs leading-5">
          <For each={props.diff.sections}>{(section) => <Section section={section} />}</For>
        </ol>
      </Show>
    </div>
  );
}
