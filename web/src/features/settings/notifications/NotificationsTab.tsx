import { Plus, RotateCw, Trash2 } from "lucide";
import { createMemo, createSignal, For, onSettled, Show } from "solid-js";
import type { NodeSummary } from "../../../api/gen/NodeSummary";
import type { NotifierList } from "../../../api/gen/NotifierList";
import type { NotifierView } from "../../../api/gen/NotifierView";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { showToast } from "../../../ui/Toast";
import { inputOf, KINDS, type Kind, MAX_NOTIFIERS } from "./draft";
import { LinkAddress } from "./LinkAddress";
import { NotifierEditor } from "./NotifierEditor";
import { KIND_ICONS, NotifierRow } from "./NotifierRow";

const t = strings.settings.notifications;
/** How often the list is read again, for the deliveries' status. */
const POLL_MS = 30_000;

/**
 * Settings › Notifications (features/observability.md, Notifiers): the notifiers, each with where
 * it sends, its filter and its last delivery; admins add, edit, test, turn on and off and remove
 * them, and set the address messages link to. Polled, since deliveries change their status.
 */
export function NotificationsTab(props: { nodes: readonly NodeSummary[]; admin: boolean }) {
  const [list, setList] = createSignal<NotifierList | undefined>();
  const [failed, setFailed] = createSignal(false);
  const [editing, setEditing] = createSignal<NotifierView | Kind | undefined>();
  const [removing, setRemoving] = createSignal<NotifierView | undefined>();
  const [busy, setBusy] = createSignal<ReadonlySet<number>>(new Set());
  const [now, setNow] = createSignal(Date.now());

  const load = (signal?: AbortSignal) =>
    api.notifiers
      .list(signal)
      .then((answer) => {
        if (!Array.isArray(answer?.notifiers)) throw new Error("not a notifier list");
        setList(answer);
        setFailed(false);
        setNow(Date.now());
      })
      .catch(() => {
        if (!signal?.aborted && list() === undefined) setFailed(true);
      });
  onSettled(() => {
    const controller = new AbortController();
    load(controller.signal);
    const timer = setInterval(() => load(controller.signal), POLL_MS);
    return () => {
      clearInterval(timer);
      controller.abort();
    };
  });

  const names = createMemo(() => new Map(props.nodes.map((n) => [n.id, n.name])));
  const notifiers = () => list()?.notifiers ?? [];
  const full = () => notifiers().length >= MAX_NOTIFIERS;
  const replace = (saved: NotifierView) =>
    setList((now) => {
      if (now === undefined) return now;
      const known = now.notifiers.some((n) => n.id === saved.id);
      return {
        ...now,
        notifiers: known
          ? now.notifiers.map((n) => (n.id === saved.id ? saved : n))
          : [...now.notifiers, saved],
      };
    });
  const run = (notifier: NotifierView, call: () => Promise<NotifierView | undefined>) => {
    setBusy((now) => new Set([...now, notifier.id]));
    call()
      .then((saved) => {
        if (saved) replace(saved);
        else
          setList(
            (now) =>
              now && { ...now, notifiers: now.notifiers.filter((n) => n.id !== notifier.id) },
          );
      })
      .catch((error: unknown) =>
        showToast({ title: t.changeFailed, description: errorText(error), tone: "crit" }),
      )
      .finally(() =>
        setBusy((now) => {
          const next = new Set(now);
          next.delete(notifier.id);
          return next;
        }),
      );
  };

  return (
    <div class="flex flex-col gap-4">
      <Show
        when={list()}
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
                  {t.retry}
                </Button>
              </div>
            </Show>
          </Card>
        }
      >
        {(loaded) => (
          <>
            <Card class="overflow-hidden" aria-label={t.listTitle}>
              <div class="flex flex-wrap items-center gap-3 border-edge border-b px-4 py-2.5">
                <div class="min-w-0">
                  <h2 class="font-medium text-sm">{t.listTitle}</h2>
                  <p role="status" class="text-fg-subtle text-xs tabular-nums">
                    {t.count(notifiers().length, notifiers().filter((n) => n.enabled).length)}
                  </p>
                </div>
                <Show
                  when={props.admin}
                  fallback={<p class="ml-auto text-fg-subtle text-xs">{t.readOnly}</p>}
                >
                  <Button
                    variant="primary"
                    icon={Plus}
                    class="ml-auto"
                    disabled={full()}
                    title={full() ? t.full(MAX_NOTIFIERS) : undefined}
                    onClick={() => setEditing("ntfy")}
                  >
                    {t.add}
                  </Button>
                </Show>
              </div>
              <Show
                when={notifiers().length > 0}
                fallback={
                  <div class="flex flex-col items-center gap-3 px-6 py-12 text-center">
                    <p class="font-medium text-fg text-sm">{t.empty}</p>
                    <p class="max-w-md text-fg-muted text-sm">{t.emptyHint}</p>
                    <Show when={props.admin}>
                      <div class="mt-2 flex flex-wrap justify-center gap-2">
                        <For each={KINDS}>
                          {(kind) => (
                            <Button onClick={() => setEditing(kind)}>
                              <Icon icon={KIND_ICONS[kind]} class="size-4" />
                              {t.addKind[kind]}
                            </Button>
                          )}
                        </For>
                      </div>
                    </Show>
                  </div>
                }
              >
                <ul>
                  <For each={notifiers()} keyed={(notifier) => notifier.id}>
                    {(notifier) => (
                      <NotifierRow
                        notifier={notifier()}
                        admin={props.admin}
                        busy={busy().has(notifier().id)}
                        now={now()}
                        nodeName={(id) => names().get(id)}
                        onToggle={(enabled) =>
                          run(notifier(), () =>
                            api.notifiers.update(notifier().id, inputOf(notifier(), enabled)),
                          )
                        }
                        onEdit={() => setEditing(notifier())}
                        onRemove={() => setRemoving(notifier())}
                      />
                    )}
                  </For>
                </ul>
              </Show>
            </Card>
            <LinkAddress
              settings={loaded().settings}
              admin={props.admin}
              onSaved={(settings) => setList((now) => now && { ...now, settings })}
            />
          </>
        )}
      </Show>
      <Show when={editing()}>
        {(target) => (
          <NotifierEditor
            notifier={typeof target() === "string" ? undefined : (target() as NotifierView)}
            kind={typeof target() === "string" ? (target() as Kind) : undefined}
            nodes={props.nodes}
            placeholders={list()?.placeholders ?? []}
            linkUrl={list()?.settings.link_url ?? null}
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
        {(notifier) => (
          <Dialog
            open
            onOpenChange={(open) => {
              if (!open) setRemoving(undefined);
            }}
            title={t.removeTitle(notifier().name)}
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
                  const target = notifier();
                  setRemoving(undefined);
                  run(target, () =>
                    api.notifiers.remove(target.id).then(() => {
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
