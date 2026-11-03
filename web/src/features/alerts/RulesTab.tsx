import { Pencil, Plus, RotateCcw, RotateCw, Trash2 } from "lucide";
import { createEffect, createMemo, createSignal, For, onSettled, Show } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { RuleView } from "../../api/gen/RuleView";
import type { Names } from "../../lib/alerts";
import { api, errorText } from "../../lib/api";
import { levelTones } from "../../lib/events";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { showToast } from "../../ui/Toast";
import { RuleEditor } from "./RuleEditor";
import { ruleSummary } from "./rules";

const t = strings.alerts.rules;

type Patterns = readonly { id: string; meaning: string }[];

/** A rule's body: what `PUT` takes. */
function specOf(rule: RuleView) {
  return {
    name: rule.name,
    enabled: rule.enabled,
    level: rule.level,
    when: rule.when,
    cooldown_s: rule.cooldown_s,
  };
}

/**
 * The rules, the user's and the built-in ones: admins turn them on and off, edit, reset and delete
 * them, and add their own. `focus` (from an alert's "Edit the rule") opens that rule's editor for
 * admins and marks its row for viewers.
 */
export function RulesTab(props: {
  names: Names;
  nodes: readonly NodeSummary[];
  admin: boolean;
  focus: number | null;
  onFocusDone: () => void;
}) {
  const [rules, setRules] = createSignal<RuleView[] | undefined>();
  const [failed, setFailed] = createSignal(false);
  const [patterns, setPatterns] = createSignal<Patterns>([]);
  const [editing, setEditing] = createSignal<RuleView | "new" | undefined>();
  const [removing, setRemoving] = createSignal<RuleView | undefined>();
  const [busy, setBusy] = createSignal<ReadonlySet<number>>(new Set());

  const load = (signal?: AbortSignal) => {
    setFailed(false);
    api.alertRules
      .list(signal)
      .then((list) => setRules(list.rules))
      .catch(() => {
        if (!signal?.aborted) setFailed(true);
      });
  };
  onSettled(() => {
    const controller = new AbortController();
    load(controller.signal);
    api.logs
      .patterns(controller.signal)
      .then((answer) => setPatterns(answer.patterns))
      .catch(() => {
        // Rules then name log patterns by their ids.
      });
    return () => controller.abort();
  });

  // An alert's "Edit the rule": admins get the editor, viewers the row marked.
  createEffect(
    () => [props.focus, rules()] as const,
    ([focus, list]) => {
      if (focus === null || list === undefined) return;
      const rule = list.find((r) => r.id === focus);
      if (rule && props.admin) {
        setEditing(rule);
        props.onFocusDone();
      } else if (rule) {
        document.getElementById(`rule-${rule.id}`)?.scrollIntoView({ block: "center" });
      }
    },
  );

  const meaning = createMemo(() => new Map(patterns().map((p) => [p.id, p.meaning])));
  const own = createMemo(() => (rules() ?? []).filter((rule) => rule.preset === null));
  const builtIn = createMemo(() => (rules() ?? []).filter((rule) => rule.preset !== null));
  const replace = (saved: RuleView) =>
    setRules((list) => {
      if (list === undefined) return [saved];
      return list.some((r) => r.id === saved.id)
        ? list.map((r) => (r.id === saved.id ? saved : r))
        : [...list, saved];
    });
  const run = (rule: RuleView, call: () => Promise<RuleView | undefined>) => {
    setBusy((now) => new Set([...now, rule.id]));
    call()
      .then((saved) => {
        if (saved) replace(saved);
        else setRules((list) => list?.filter((r) => r.id !== rule.id));
      })
      .catch((error: unknown) =>
        showToast({ title: t.changeFailed, description: errorText(error), tone: "crit" }),
      )
      .finally(() =>
        setBusy((now) => {
          const next = new Set(now);
          next.delete(rule.id);
          return next;
        }),
      );
  };

  const row = (rule: RuleView) => (
    <li
      id={`rule-${rule.id}`}
      class={[
        "flex items-start gap-3 border-edge/60 border-b px-4 py-3 last:border-b-0",
        { "bg-accent/5 ring-1 ring-accent/40 ring-inset": props.focus === rule.id && !props.admin },
      ]}
    >
      <div class="mt-0.5 w-9 shrink-0">
        <Show
          when={props.admin}
          fallback={
            <span class={["text-xs", rule.enabled ? "text-fg" : "text-fg-subtle"]}>
              {rule.enabled ? t.on : t.off}
            </span>
          }
        >
          <Checkbox
            label={rule.enabled ? t.disable(rule.name) : t.enable(rule.name)}
            hideLabel
            checked={rule.enabled}
            disabled={busy().has(rule.id)}
            onChange={(enabled) =>
              run(rule, () => api.alertRules.update(rule.id, { ...specOf(rule), enabled }))
            }
          />
        </Show>
      </div>
      <div class={["flex min-w-0 flex-1 flex-col gap-1", { "opacity-60": !rule.enabled }]}>
        <div class="flex flex-wrap items-center gap-2">
          <span class="font-medium text-fg text-sm">{rule.name}</span>
          <Badge tone={levelTones[rule.level]}>{strings.events.levelName[rule.level]}</Badge>
          <Show when={rule.modified}>
            <Badge>{t.modified}</Badge>
          </Show>
        </div>
        <p class="text-fg-muted text-xs">
          <span class="text-fg-subtle">{rule.when.type === "metric" ? t.metric : t.event} · </span>
          {ruleSummary(rule, props.names, (id) => meaning().get(id))}
        </p>
      </div>
      <Show when={props.admin}>
        <div class="flex shrink-0 items-center gap-1">
          <Button
            size="sm"
            variant="ghost"
            icon={Pencil}
            aria-label={`${t.edit}: ${rule.name}`}
            onClick={() => setEditing(rule)}
          >
            <span class="max-sm:hidden">{t.edit}</span>
          </Button>
          <Show when={rule.preset !== null && rule.modified}>
            <Button
              size="sm"
              variant="ghost"
              icon={RotateCcw}
              title={t.resetHint}
              aria-label={`${t.reset}: ${rule.name}`}
              disabled={busy().has(rule.id)}
              onClick={() => run(rule, () => api.alertRules.reset(rule.id))}
            />
          </Show>
          <Show when={rule.preset === null}>
            <Button
              size="sm"
              variant="ghost"
              icon={Trash2}
              aria-label={`${t.remove}: ${rule.name}`}
              disabled={busy().has(rule.id)}
              onClick={() => setRemoving(rule)}
            />
          </Show>
        </div>
      </Show>
    </li>
  );

  return (
    <div class="flex flex-col gap-4">
      <Show
        when={rules()}
        fallback={
          <Card>
            <Show
              when={failed()}
              fallback={
                <p role="status" class="py-20 text-center text-fg-subtle text-sm">
                  {t.loading}…
                </p>
              }
            >
              <div role="alert" class="flex flex-col items-center gap-3 py-16 text-sm">
                <p class="text-fg-muted">{t.failed}</p>
                <Button size="sm" icon={RotateCw} onClick={() => load()}>
                  {strings.alerts.history.retry}
                </Button>
              </div>
            </Show>
          </Card>
        }
      >
        {(list) => (
          <>
            <div class="flex flex-wrap items-center gap-3">
              <p role="status" class="text-fg-muted text-sm tabular-nums">
                {t.count(list().length, list().filter((r) => r.enabled).length)}
              </p>
              <Show
                when={props.admin}
                fallback={<p class="ml-auto text-fg-subtle text-xs">{t.readOnly}</p>}
              >
                <Button
                  variant="primary"
                  icon={Plus}
                  class="ml-auto"
                  onClick={() => setEditing("new")}
                >
                  {t.add}
                </Button>
              </Show>
            </div>
            <Show when={own().length > 0}>
              <Card class="overflow-hidden" aria-label={t.own}>
                <h2 class="border-edge border-b px-4 py-2.5 font-medium text-sm">{t.own}</h2>
                <ul>
                  <For each={own()}>{row}</For>
                </ul>
              </Card>
            </Show>
            <Card class="overflow-hidden" aria-label={t.builtIn}>
              <div class="border-edge border-b px-4 py-2.5">
                <h2 class="font-medium text-sm">{t.builtIn}</h2>
                <p class="text-fg-subtle text-xs">{t.builtInHint}</p>
              </div>
              <ul>
                <For each={builtIn()}>{row}</For>
              </ul>
            </Card>
          </>
        )}
      </Show>
      <Show when={editing()}>
        {(target) => (
          <RuleEditor
            rule={target() === "new" ? undefined : (target() as RuleView)}
            nodes={props.nodes}
            patterns={patterns()}
            onClose={() => setEditing(undefined)}
            onSaved={(saved) => {
              replace(saved);
              setEditing(undefined);
              showToast({ title: t.saved(saved.name), tone: "ok" });
            }}
          />
        )}
      </Show>
      <Show when={removing()}>
        {(rule) => (
          <Dialog
            open
            onOpenChange={(open) => {
              if (!open) setRemoving(undefined);
            }}
            title={t.removeTitle(rule().name)}
            description={t.removeDetail}
          >
            <div class="flex justify-end gap-2 px-5 pt-2 pb-4">
              <Button variant="ghost" onClick={() => setRemoving(undefined)}>
                {t.cancel}
              </Button>
              <Button
                variant="primary"
                icon={Trash2}
                onClick={() => {
                  const target = rule();
                  setRemoving(undefined);
                  run(target, () =>
                    api.alertRules.remove(target.id).then(() => {
                      showToast({ title: t.removed(target.name), tone: "info" });
                      return undefined;
                    }),
                  );
                }}
              >
                {t.remove}
              </Button>
            </div>
          </Dialog>
        )}
      </Show>
    </div>
  );
}
