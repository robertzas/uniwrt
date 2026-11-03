import { ArrowUpRight, BellOff, BellRing, Check, SlidersHorizontal } from "lucide";
import { createSignal, Show } from "solid-js";
import type { AlertRecord } from "../../api/gen/AlertRecord";
import {
  alertPage,
  causeText,
  eventsAround,
  firingFor,
  type Names,
  subjectIcon,
  subjectLabel,
} from "../../lib/alerts";
import { levelTones } from "../../lib/events";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { SnoozeMenu } from "./SnoozeMenu";
import { ACTION, acknowledge, Field, full, TILES } from "./shared";

const t = strings.alerts;

const time = new Intl.DateTimeFormat(undefined, { timeStyle: "short" });
const date = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

/** `14:05` today, else `Oct 31 14:05`. */
function startLabel(ms: number, now: number): string {
  const at = new Date(ms);
  return at.toDateString() === new Date(now).toDateString()
    ? time.format(at)
    : `${date.format(at)} ${time.format(at)}`;
}

export interface AlertRowProps {
  alert: AlertRecord;
  names: Names;
  now: number;
  admin: boolean;
  expanded: boolean;
  onToggle: () => void;
  /** Marks an alert still firing (the history does; on the Active tab they all are). */
  markFiring?: boolean;
}

/** One alert: a line with its icon, rule, subject, cause and state, expanding into details. */
export function AlertRow(props: AlertRowProps) {
  const a = () => props.alert;
  const active = () => a().resolved_at === null;
  const dim = () => active() && (a().acknowledged !== null || a().silenced);
  const subject = () => subjectLabel(a(), props.names);
  return (
    <li
      id={`alert-${a().id}`}
      class={["border-edge/60 border-b", props.expanded ? "bg-raised/60" : "hover:bg-raised/40"]}
    >
      <div class="flex items-start gap-2 pr-3">
        <button
          type="button"
          aria-expanded={props.expanded ? "true" : "false"}
          onClick={() => props.onToggle()}
          class="flex min-w-0 flex-1 items-start gap-3 py-2.5 pl-4 text-left focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
        >
          <span
            class={[
              "mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg border",
              active() ? TILES[levelTones[a().level]] : TILES.neutral,
              { "opacity-60": dim() },
            ]}
          >
            <Icon icon={subjectIcon(a())} class="size-4" />
            <span class="sr-only">{strings.events.levelName[a().level]}</span>
          </span>
          <span class={["flex min-w-0 flex-1 flex-col gap-0.5", { "opacity-70": dim() }]}>
            <span class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
              <span class="min-w-0 font-medium text-fg text-sm">{a().name}</span>
              <Show when={a().count > 1}>
                <span
                  title={t.firedHint(a().count)}
                  class="rounded-full border border-edge bg-raised px-1.5 font-medium font-mono text-[0.6875rem] text-fg-muted tabular-nums"
                >
                  {t.fired(a().count)}
                </span>
              </Show>
              <State alert={a()} markFiring={props.markFiring ?? true} />
            </span>
            <span class="truncate text-fg-muted text-xs">
              {subject()}
              <Show when={causeText(a())}>{(cause) => <> · {cause()}</>}</Show>
            </span>
          </span>
          <span class="mt-0.5 flex shrink-0 flex-col items-end gap-0.5 text-xs">
            <time
              datetime={new Date(a().started_at).toISOString()}
              title={full.format(new Date(a().started_at))}
              class="font-mono text-fg-subtle tabular-nums"
            >
              {startLabel(a().started_at, props.now)}
            </time>
            <span class="text-fg-subtle tabular-nums">
              {active() ? t.firingFor(firingFor(a(), props.now)) : t.lasted(firingFor(a()))}
            </span>
          </span>
        </button>
        <Show when={props.admin && active() && a().acknowledged === null}>
          <Acknowledge alert={a()} subject={subject()} />
        </Show>
      </div>
      <Show when={props.expanded}>
        <Details {...props} />
      </Show>
    </li>
  );
}

/** Its state as a badge: resolved (and why), silenced, acknowledged, or firing. */
function State(props: { alert: AlertRecord; markFiring: boolean }) {
  const pill = "inline-flex items-center gap-1 rounded-full border px-1.5 text-[0.6875rem]";
  return (
    <Show
      when={props.alert.resolved_at === null}
      fallback={
        <span class={[pill, "border-edge bg-raised text-fg-muted"]}>
          {t.state.resolved}
          <Show when={props.alert.resolution}>{(why) => <> · {t.resolution[why()]}</>}</Show>
        </span>
      }
    >
      <Show when={props.alert.silenced}>
        <span title={t.silencedHint} class={[pill, "border-edge bg-raised text-fg-muted"]}>
          <Icon icon={BellOff} class="size-3" />
          {t.state.silenced}
        </span>
      </Show>
      <Show
        when={props.alert.acknowledged === null}
        fallback={
          <span class={[pill, "border-edge bg-raised text-fg-muted"]}>
            <Icon icon={Check} class="size-3" />
            {t.state.acknowledged}
          </span>
        }
      >
        <Show when={props.markFiring}>
          <span class={[pill, "border-crit/30 bg-crit/10 font-medium text-fg"]}>
            <span class="size-1.5 animate-pulse rounded-full bg-crit" />
            {t.state.firing}
          </span>
        </Show>
      </Show>
    </Show>
  );
}

function Acknowledge(props: { alert: AlertRecord; subject: string }) {
  const [busy, setBusy] = createSignal(false);
  return (
    <Button
      size="sm"
      variant="ghost"
      icon={Check}
      disabled={busy()}
      title={t.acknowledgeHint}
      aria-label={`${t.acknowledge}: ${props.alert.name}, ${props.subject}`}
      class="mt-2.5"
      onClick={() => {
        setBusy(true);
        void acknowledge(props.alert.id).finally(() => setBusy(false));
      }}
    >
      <span class="max-sm:hidden">{t.acknowledge}</span>
    </Button>
  );
}

function Details(props: AlertRowProps) {
  const a = () => props.alert;
  const f = t.field;
  const page = () => alertPage(a());
  const condition = () => {
    const cause = a().cause;
    return cause.type === "event" ? cause.condition : null;
  };
  const node = () => {
    const id = a().node;
    return id === null ? undefined : { id, name: props.names.node(id) ?? strings.events.node(id) };
  };
  return (
    <div class="flex flex-col gap-3 pr-4 pb-4 pl-4 sm:pl-15">
      <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-xs sm:grid-cols-[auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
        <Field label={f.started}>
          <span class="font-mono tabular-nums">{full.format(new Date(a().started_at))}</span>
        </Field>
        <Field label={f.lastAt}>
          <span class="font-mono tabular-nums">{full.format(new Date(a().last_at))}</span>
        </Field>
        <Show when={a().resolved_at}>
          {(at) => (
            <Field label={f.resolved}>
              <span class="font-mono tabular-nums">{full.format(new Date(at()))}</span>
              <Show when={a().resolution}>
                {(why) => <span class="text-fg-subtle"> · {t.resolution[why()]}</span>}
              </Show>
            </Field>
          )}
        </Show>
        <Show when={a().count > 1}>
          <Field label={f.count}>{a().count.toLocaleString()}</Field>
        </Show>
        <Field label={f.level}>{strings.events.levelName[a().level]}</Field>
        <Field label={f.subject}>
          <Show when={page()} fallback={subjectLabel(a(), props.names)}>
            {(href) => (
              <a href={href()} class="font-medium text-accent hover:underline">
                {subjectLabel(a(), props.names)}
              </a>
            )}
          </Show>
        </Field>
        <Show when={causeText(a())}>{(cause) => <Field label={f.cause}>{cause()}</Field>}</Show>
        <Show when={condition()}>
          {(key) => (
            <Field label={f.condition}>
              <span class="font-mono">{key()}</span>
            </Field>
          )}
        </Show>
        <Show when={a().acknowledged}>
          {(ack) => (
            <Field label={t.state.acknowledged}>
              {t.acknowledgedBy(ack().username, full.format(new Date(ack().at)))}
            </Field>
          )}
        </Show>
        <Show when={a().silenced}>
          <Field label={t.state.silenced}>{t.silencedHint}</Field>
        </Show>
      </dl>
      <div class="flex flex-wrap gap-2">
        <Show when={page()}>
          {(href) => (
            <a href={href()} class={ACTION}>
              <Icon icon={ArrowUpRight} class="size-3.5 text-fg-subtle" />
              {a().node === null && a().client !== null ? t.actions.client : t.actions.device}
            </a>
          )}
        </Show>
        <a href={eventsAround(a())} class={ACTION}>
          <Icon icon={BellRing} class="size-3.5 text-fg-subtle" />
          {t.actions.events}
        </a>
        <a href={`/insights/alerts?tab=rules&rule=${a().rule}`} class={ACTION}>
          <Icon icon={SlidersHorizontal} class="size-3.5 text-fg-subtle" />
          {t.actions.rule}
        </a>
        <Show when={props.admin && a().resolved_at === null}>
          <SnoozeMenu node={node()} />
        </Show>
      </div>
    </div>
  );
}
