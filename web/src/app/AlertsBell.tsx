import * as Popover from "@kobalte/core/popover";
import { Bell, BellOff, Check, CircleCheck } from "lucide";
import { createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { AlertRecord } from "../api/gen/AlertRecord";
import {
  byAttention,
  causeText,
  firingFor,
  type Names,
  subjectIcon,
  subjectLabel,
  unattended,
  worstLevel,
} from "../lib/alerts";
import { api } from "../lib/api";
import { levelTones } from "../lib/events";
import { LiveContext } from "../lib/live";
import { SessionContext } from "../lib/session";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { showToast } from "../ui/Toast";

const t = strings.alerts;
/** The alerts the popover lists; the rest are on the alerts page. */
export const SHOWN = 8;

/** The count's badge, by the worst level among the alerts wanting attention. */
const BADGES = {
  crit: "bg-crit text-canvas",
  warn: "bg-warn text-canvas",
  info: "bg-info text-canvas",
  neutral: "bg-fg-subtle text-canvas",
} as const;

const TILES = {
  crit: "bg-crit/10 text-crit",
  warn: "bg-warn/10 text-warn",
  info: "bg-info/10 text-info",
  neutral: "bg-raised text-fg-subtle",
} as const;

/**
 * The top bar's bell (features/observability.md, Alerts): how many alerts want attention (firing,
 * neither acknowledged nor silenced), tinted by the worst, and a popover of the alerts firing,
 * which admins acknowledge there.
 */
export function AlertsBell() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const [open, setOpen] = createSignal(false);
  const [now, setNow] = createSignal(Date.now());
  const pending = createMemo(() => unattended(live.alerts.active));
  const tone = () => {
    const worst = worstLevel(pending());
    return worst === undefined ? "neutral" : levelTones[worst];
  };
  const shown = createMemo(() => byAttention(live.alerts.active));
  const nodeNames = createMemo(() => new Map(live.fleet.nodes.map((n) => [n.id, n.name])));
  const names: Names = { node: (id) => nodeNames().get(id), client: () => undefined };

  onSettled(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  return (
    <Popover.Root
      open={open()}
      onOpenChange={(next) => {
        setNow(Date.now());
        setOpen(next);
      }}
      placement="bottom-end"
      gutter={8}
    >
      <Popover.Trigger
        aria-label={t.bell.label(pending().length)}
        class="relative grid size-8 place-items-center rounded-control text-fg-muted transition-colors hover:bg-raised hover:text-fg data-expanded:bg-raised data-expanded:text-fg"
      >
        <Icon icon={Bell} class="size-4.5" />
        <Show
          when={pending().length > 0}
          fallback={
            <Show when={live.alerts.active.length > 0}>
              <span
                aria-hidden="true"
                class="absolute top-1.5 right-1.5 size-2 rounded-full border border-canvas bg-fg-subtle"
              />
            </Show>
          }
        >
          <span
            aria-hidden="true"
            class={[
              "absolute -top-0.5 -right-0.5 h-4 min-w-4 rounded-full px-1 text-center font-semibold text-[10px] leading-4 tabular-nums ring-2 ring-canvas",
              BADGES[tone()],
            ]}
          >
            {pending().length > 99 ? "99+" : pending().length}
          </span>
        </Show>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 flex max-h-[min(34rem,75vh)] w-[min(24rem,calc(100vw-2rem))] flex-col rounded-card border border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
          <div class="flex items-baseline justify-between gap-3 border-edge border-b px-4 py-3">
            <Popover.Title class="font-semibold text-sm">{t.title}</Popover.Title>
            <span class="text-fg-subtle text-xs tabular-nums">
              {t.bell.firing(live.alerts.active.length)}
            </span>
          </div>
          <Show
            when={shown().length > 0}
            fallback={
              <div class="flex flex-col items-center gap-2 px-6 py-8 text-center">
                <span class="grid size-10 place-items-center rounded-xl bg-ok/10 text-ok">
                  <Icon icon={CircleCheck} class="size-5" />
                </span>
                <p class="font-medium text-sm">{t.bell.quiet}</p>
                <p class="text-fg-subtle text-xs">{t.bell.quietDetail}</p>
              </div>
            }
          >
            <ul class="min-h-0 overflow-y-auto p-1.5">
              <For each={shown().slice(0, SHOWN)}>
                {(alert) => (
                  <BellRow
                    alert={alert}
                    names={names}
                    now={now()}
                    admin={session.user.role === "admin"}
                    onOpen={() => setOpen(false)}
                  />
                )}
              </For>
            </ul>
          </Show>
          <div class="flex items-center justify-between gap-3 border-edge border-t px-4 py-2.5 text-sm">
            <a
              href="/insights/alerts"
              onClick={() => setOpen(false)}
              class="font-medium text-accent hover:underline"
            >
              {t.bell.all}
            </a>
            <Show when={shown().length > SHOWN}>
              <span class="text-fg-subtle text-xs">{t.bell.more(shown().length - SHOWN)}</span>
            </Show>
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

function BellRow(props: {
  alert: AlertRecord;
  names: Names;
  now: number;
  admin: boolean;
  onOpen: () => void;
}) {
  const [busy, setBusy] = createSignal(false);
  const a = () => props.alert;
  const dim = () => a().acknowledged !== null || a().silenced;
  const acknowledge = () => {
    setBusy(true);
    api.alerts
      .acknowledge(a().id)
      .catch((error: unknown) =>
        showToast({
          title: t.acknowledgeFailed,
          description: error instanceof Error ? error.message : undefined,
          tone: "crit",
        }),
      )
      .finally(() => setBusy(false));
  };
  return (
    <li
      class={[
        "group flex items-start gap-1 rounded-control hover:bg-raised",
        { "opacity-60": dim() },
      ]}
    >
      <a
        href={`/insights/alerts?alert=${a().id}`}
        onClick={() => props.onOpen()}
        class="flex min-w-0 flex-1 items-start gap-3 px-2 py-2"
      >
        <span
          class={[
            "mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg",
            TILES[levelTones[a().level]],
          ]}
        >
          <Icon icon={subjectIcon(a())} class="size-4" />
          <span class="sr-only">{strings.events.levelName[a().level]}</span>
        </span>
        <span class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="truncate font-medium text-fg text-sm">{a().name}</span>
          <span class="truncate text-fg-muted text-xs">
            {subjectLabel(a(), props.names)}
            <Show when={causeText(a())}>{(cause) => <> · {cause()}</>}</Show>
          </span>
          <span class="flex items-center gap-1.5 text-fg-subtle text-xs">
            <Show when={a().silenced}>
              <Icon icon={BellOff} class="size-3" />
              <span>{t.state.silenced} ·</span>
            </Show>
            <Show when={a().acknowledged}>
              <Icon icon={Check} class="size-3" />
              <span>{t.state.acknowledged} ·</span>
            </Show>
            <span class="tabular-nums">{t.firingFor(firingFor(a(), props.now))}</span>
          </span>
        </span>
      </a>
      <Show when={props.admin && a().acknowledged === null}>
        <button
          type="button"
          disabled={busy()}
          onClick={acknowledge}
          aria-label={`${t.acknowledge}: ${a().name}, ${subjectLabel(a(), props.names)}`}
          title={t.acknowledgeHint}
          class="mt-2 mr-1.5 grid size-7 shrink-0 place-items-center rounded-control text-fg-subtle hover:bg-surface hover:text-fg focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
        >
          <Icon icon={Check} class="size-4" />
        </button>
      </Show>
    </li>
  );
}
