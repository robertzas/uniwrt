import type { JSX } from "@solidjs/web";
import { ArrowUpRight, Filter, ScrollText } from "lucide";
import { Show } from "solid-js";
import type { EventRecord } from "../../api/gen/EventRecord";
import { categoryIcons, type Described, levelTones, logsAround } from "../../lib/events";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";

const t = strings.events;

const time = new Intl.DateTimeFormat(undefined, { timeStyle: "medium" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "medium" });

/** The tile behind a category's icon, tinted by the level. */
const TILES = {
  crit: "border-crit/30 bg-crit/10 text-crit",
  warn: "border-warn/30 bg-warn/10 text-warn",
  info: "border-info/30 bg-info/10 text-info",
  neutral: "border-edge bg-raised text-fg-subtle",
} as const;

export interface EventRowProps {
  event: EventRecord;
  described: Described;
  /** The device's and the client's names and pages, when the event names them. */
  device?: { name: string; href: string } | undefined;
  client?: { name: string; href: string } | undefined;
  /** A condition it raised that nothing cleared yet. */
  ongoing: boolean;
  expanded: boolean;
  onToggle: () => void;
  onOnlyDevice: () => void;
  onOnlyCategory: () => void;
}

/** One event: a line with its icon, title and time, which expands into details and actions. */
export function EventRow(props: EventRowProps) {
  const e = () => props.event;
  const tone = () => levelTones[e().level];
  return (
    <li class={["border-edge/60 border-b", props.expanded ? "bg-raised/60" : "hover:bg-raised/40"]}>
      <button
        type="button"
        aria-expanded={props.expanded ? "true" : "false"}
        onClick={() => props.onToggle()}
        class="flex w-full items-start gap-3 px-4 py-2.5 text-left focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <span
          class={[
            "mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg border",
            TILES[tone()],
          ]}
        >
          <Icon icon={categoryIcons[e().category]} class="size-4" />
          <span class="sr-only">
            {t.levelName[e().level]}, {t.category[e().category]}
          </span>
        </span>
        <span class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <span class="min-w-0 font-medium text-fg text-sm">{props.described.title}</span>
            <Show when={e().count > 1}>
              <span
                title={t.repeatedHint(e().count, full.format(new Date(e().last_at)))}
                class="rounded-full border border-edge bg-raised px-1.5 font-medium font-mono text-[0.6875rem] text-fg-muted tabular-nums"
              >
                {t.repeated(e().count)}
              </span>
            </Show>
            <Show when={props.ongoing}>
              <span class="inline-flex items-center gap-1 rounded-full border border-crit/30 bg-crit/10 px-1.5 font-medium text-[0.6875rem] text-fg">
                <span class="size-1.5 animate-pulse rounded-full bg-crit" />
                {t.ongoing}
              </span>
            </Show>
          </span>
          <Show when={props.described.detail}>
            {(detail) => (
              <span
                class={[
                  "text-fg-muted text-xs",
                  props.expanded ? "whitespace-pre-wrap [overflow-wrap:anywhere]" : "truncate",
                ]}
              >
                {detail()}
              </span>
            )}
          </Show>
        </span>
        <time
          datetime={new Date(e().at).toISOString()}
          title={full.format(new Date(e().at))}
          class="mt-0.5 shrink-0 font-mono text-fg-subtle text-xs tabular-nums"
        >
          {time.format(new Date(e().at))}
        </time>
      </button>
      <Show when={props.expanded}>
        <Details {...props} />
      </Show>
    </li>
  );
}

function Details(props: EventRowProps) {
  const e = () => props.event;
  const f = t.field;
  return (
    <div class="flex flex-col gap-3 pr-4 pb-4 pl-4 sm:pl-15">
      <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-xs sm:grid-cols-[auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
        <Field label={f.when}>
          <span class="font-mono tabular-nums">{full.format(new Date(e().at))}</span>
        </Field>
        <Show when={e().count > 1}>
          <Field label={f.lastAt}>
            <span class="font-mono tabular-nums">{full.format(new Date(e().last_at))}</span>
          </Field>
          <Field label={f.count}>{e().count.toLocaleString()}</Field>
        </Show>
        <Field label={f.level}>{t.levelName[e().level]}</Field>
        <Field label={f.category}>{t.category[e().category]}</Field>
        <Show when={props.device}>
          {(device) => (
            <Field label={f.device}>
              <a href={device().href} class="font-medium text-accent hover:underline">
                {device().name}
              </a>
            </Field>
          )}
        </Show>
        <Show when={props.client}>
          {(client) => (
            <Field label={f.client}>
              <a href={client().href} class="font-medium text-accent hover:underline">
                {client().name}
              </a>
            </Field>
          )}
        </Show>
        <Show when={e().condition}>
          {(condition) => (
            <Field label={f.condition}>
              <span class="font-mono">{condition().key}</span>
              <span class="text-fg-subtle">
                {" "}
                · {condition().raised ? (props.ongoing ? t.ongoing : t.raised) : t.resolved}
              </span>
            </Field>
          )}
        </Show>
      </dl>
      <div class="flex flex-wrap gap-2">
        <a
          href={logsAround(e())}
          class="inline-flex h-7 items-center gap-1.5 rounded-control border border-edge bg-surface px-2.5 font-medium text-fg text-xs hover:bg-raised"
        >
          <Icon icon={ScrollText} class="size-3.5 text-fg-subtle" />
          {t.actions.logs}
        </a>
        <Show when={props.device}>
          {(device) => (
            <a
              href={device().href}
              class="inline-flex h-7 items-center gap-1.5 rounded-control border border-edge bg-surface px-2.5 font-medium text-fg text-xs hover:bg-raised"
            >
              <Icon icon={ArrowUpRight} class="size-3.5 text-fg-subtle" />
              {t.actions.device}
            </a>
          )}
        </Show>
        <Show when={props.client}>
          {(client) => (
            <a
              href={client().href}
              class="inline-flex h-7 items-center gap-1.5 rounded-control border border-edge bg-surface px-2.5 font-medium text-fg text-xs hover:bg-raised"
            >
              <Icon icon={ArrowUpRight} class="size-3.5 text-fg-subtle" />
              {t.actions.client}
            </a>
          )}
        </Show>
        <Show when={e().node !== null}>
          <Button size="sm" variant="ghost" icon={Filter} onClick={() => props.onOnlyDevice()}>
            {t.actions.onlyDevice}
          </Button>
        </Show>
        <Button size="sm" variant="ghost" icon={Filter} onClick={() => props.onOnlyCategory()}>
          {t.actions.onlyCategory}
        </Button>
      </div>
    </div>
  );
}

function Field(props: { label: string; children: JSX.Element }) {
  return (
    <>
      <dt class="text-fg-subtle">{props.label}</dt>
      <dd class="min-w-0 text-fg">{props.children}</dd>
    </>
  );
}
