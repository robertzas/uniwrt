import { FilePlus2, History, Pencil, RotateCcw, RotateCw } from "lucide";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { RevisionDiff } from "../../../api/gen/RevisionDiff";
import type { RevisionKind } from "../../../api/gen/RevisionKind";
import type { RevisionMeta } from "../../../api/gen/RevisionMeta";
import { api } from "../../../lib/api";
import { formatDuration } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { SELECT } from "../../../ui/FormField";
import { Icon, type IconNode } from "../../../ui/Icon";
import { ChangeList } from "./ChangeList";
import { counts, title } from "./changes";
import { RestoreDialog } from "./RestoreDialog";

const t = strings.settings.revisions;

const KIND_ICONS: Record<RevisionKind, IconNode> = {
  defaults: FilePlus2,
  edit: Pencil,
  restore: History,
};

const time = new Intl.DateTimeFormat(undefined, { timeStyle: "short" });
const date = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });
const full = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "medium" });

/** `14:05` today, else `Oct 31 14:05`. */
function when(ms: number, now: number): string {
  const at = new Date(ms);
  return at.toDateString() === new Date(now).toDateString()
    ? time.format(at)
    : `${date.format(at)} ${time.format(at)}`;
}

/** `5m ago`, or `just now` within a minute. */
function ago(ms: number, now: number): string {
  const seconds = (now - ms) / 1000;
  return seconds < 60 ? t.justNow : t.ago(formatDuration(seconds));
}

export interface RevisionRowProps {
  revision: RevisionMeta;
  latest: boolean;
  expanded: boolean;
  onToggle: () => void;
  /** Every revision listed, to compare with. */
  revisions: readonly RevisionMeta[];
  now: number;
  nodeName: (id: number) => string | undefined;
  /** Admins restore revisions other than the latest. */
  admin: boolean;
  /** The latest revision's id, the base of a restore. */
  latestId: number;
}

/** One revision: its kind, title, author, time and size, opening into its changes. */
export function RevisionRow(props: RevisionRowProps) {
  const r = () => props.revision;
  return (
    <li
      id={`revision-${r().id}`}
      class={["border-edge/60 border-b", props.expanded ? "bg-raised/60" : "hover:bg-raised/40"]}
    >
      <button
        type="button"
        aria-expanded={props.expanded ? "true" : "false"}
        aria-label={t.toggle(r().id)}
        onClick={() => props.onToggle()}
        class="flex w-full min-w-0 items-start gap-3 px-4 py-2.5 text-left focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <span class="mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg border border-edge bg-raised text-fg-muted">
          <Icon icon={KIND_ICONS[r().kind]} class="size-4" />
          <span class="sr-only">{t.kind[r().kind]}</span>
        </span>
        <span class="flex min-w-0 flex-1 flex-col gap-0.5">
          <span class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <span class="font-medium font-mono text-fg-subtle text-xs tabular-nums">
              {t.number(r().id)}
            </span>
            <span class="min-w-0 truncate font-medium text-fg text-sm">{title(r())}</span>
            <Show when={props.latest}>
              <span class="inline-flex items-center rounded-full border border-accent/40 bg-accent/10 px-1.5 text-[0.6875rem] text-fg">
                {t.latest}
              </span>
            </Show>
          </span>
          <span class="truncate text-fg-muted text-xs">
            {r().author === null ? t.byController : t.by(r().author ?? "")}
            <Show when={r().restored_from}>{(from) => <> · {t.restoredFrom(from())}</>}</Show>
            {" · "}
            <span class="tabular-nums">{t.changes(r().changes)}</span>
          </span>
        </span>
        <span class="mt-0.5 flex shrink-0 flex-col items-end gap-0.5 text-xs">
          <time
            datetime={new Date(r().created_at).toISOString()}
            title={full.format(new Date(r().created_at))}
            class="font-mono text-fg-subtle tabular-nums"
          >
            {when(r().created_at, props.now)}
          </time>
          <span class="text-fg-subtle tabular-nums">{ago(r().created_at, props.now)}</span>
        </span>
      </button>
      <Show when={props.expanded}>
        <Details {...props} />
      </Show>
    </li>
  );
}

/** The changes from the revision to compare with (its parent at first) to this one. */
function Details(props: RevisionRowProps) {
  const [against, setAgainst] = createSignal<number | null>(null);
  const [diff, setDiff] = createSignal<RevisionDiff | undefined>();
  const [failed, setFailed] = createSignal(false);
  const [attempt, setAttempt] = createSignal(0);
  const others = createMemo(() =>
    props.revisions.filter((r) => r.id !== props.revision.id && r.id !== props.revision.parent),
  );
  const selectId = () => `revision-${props.revision.id}-against`;
  const [restoring, setRestoring] = createSignal(false);

  createEffect(
    () => [props.revision.id, against(), attempt()] as const,
    ([id, other]) => {
      const controller = new AbortController();
      setDiff(undefined);
      setFailed(false);
      api.revisions
        .diff(id, other, controller.signal)
        .then(setDiff)
        .catch(() => {
          if (!controller.signal.aborted) setFailed(true);
        });
      return () => controller.abort();
    },
  );

  const summary = () => {
    const changes = diff()?.changes ?? [];
    const c = counts(changes);
    return t.opCounts(c.added, c.removed, c.changed);
  };

  return (
    <div class="flex flex-col gap-3 px-4 pt-1 pb-4 sm:pl-15">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
        <label for={selectId()} class="text-fg-muted text-xs">
          {t.compare}
        </label>
        <div class="w-full min-w-0 sm:w-72">
          <select
            id={selectId()}
            class={[SELECT, "h-8 text-xs"]}
            value={against() === null ? "" : String(against())}
            onChange={(e) => {
              const value = e.currentTarget.value;
              setAgainst(value === "" ? null : Number(value));
            }}
          >
            <option value="">
              {props.revision.parent === null ? t.nothing : t.parent(props.revision.parent)}
            </option>
            <For each={others()}>
              {(other) => (
                <option value={String(other.id)}>{t.option(other.id, title(other))}</option>
              )}
            </For>
          </select>
        </div>
        <Show when={diff()?.from}>
          {(from) => (
            <span class="text-fg-subtle text-xs tabular-nums">
              {t.from(from(), props.revision.id)}
              <Show when={summary()}>{(text) => <> · {text()}</>}</Show>
            </span>
          )}
        </Show>
        <Show when={props.admin && !props.latest}>
          <span class="sm:ml-auto">
            <Button
              size="sm"
              icon={RotateCcw}
              aria-label={t.restore.label(props.revision.id)}
              onClick={() => setRestoring(true)}
            >
              {t.restore.action}
            </Button>
          </span>
          <RestoreDialog
            id={props.revision.id}
            base={props.latestId}
            open={restoring()}
            onOpenChange={setRestoring}
          />
        </Show>
      </div>
      <Show
        when={diff()}
        fallback={
          <Show
            when={failed()}
            fallback={
              <p role="status" class="py-4 text-fg-subtle text-sm">
                {t.diffLoading}…
              </p>
            }
          >
            <div role="alert" class="flex items-center gap-3 py-2 text-sm">
              <p class="text-fg-muted">{t.diffFailed}</p>
              <Button size="sm" icon={RotateCw} onClick={() => setAttempt((n) => n + 1)}>
                {t.retry}
              </Button>
            </div>
          </Show>
        }
      >
        {(loaded) => (
          <Show
            when={loaded().changes.length > 0}
            fallback={
              <p class="py-2 text-fg-muted text-sm">{loaded().from === null ? t.first : t.same}</p>
            }
          >
            <ChangeList changes={loaded().changes} nodeName={props.nodeName} />
          </Show>
        )}
      </Show>
    </div>
  );
}
