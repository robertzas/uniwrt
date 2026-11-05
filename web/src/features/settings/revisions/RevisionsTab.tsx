import { useNavigate } from "@solidjs/router";
import { Download, RotateCw } from "lucide";
import { createEffect, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { RevisionMeta } from "../../../api/gen/RevisionMeta";
import { api } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { merge } from "./changes";
import { RevisionRow } from "./RevisionRow";

const t = strings.settings.revisions;
/** Revisions per page. */
const PAGE = 50;

/**
 * Settings › Revisions (features/lifecycle.md, Desired state & revisions): the site's history,
 * newest first with older pages on demand; a revision opens into its changes from its parent or
 * any other revision listed; admins restore one as the next revision. The newest page is read
 * again whenever the `config` topic names a new latest revision (D-071).
 */
export function RevisionsTab(props: {
  nodeName: (id: number) => string | undefined;
  admin: boolean;
}) {
  const live = useContext(LiveContext);
  const navigate = useNavigate();
  const [revisions, setRevisions] = createSignal<readonly RevisionMeta[] | undefined>();
  const [more, setMore] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  const [older, setOlder] = createSignal<"idle" | "loading" | "failed">("idle");
  const [open, setOpen] = createSignal<number | undefined>();
  const [now, setNow] = createSignal(Date.now());

  const load = (signal?: AbortSignal) =>
    api.revisions
      .page(null, PAGE, signal)
      .then((page) => {
        if (!Array.isArray(page?.revisions)) throw new Error("not a revision page");
        const known = revisions();
        // The older tail never changes (revisions are immutable), so `more` holds once known.
        if (known === undefined) setMore(page.more);
        setRevisions(merge(known ?? [], page.revisions));
        setFailed(false);
        setNow(Date.now());
      })
      .catch(() => {
        if (!signal?.aborted && revisions() === undefined) setFailed(true);
      });
  const controller = new AbortController();
  onSettled(() => {
    load(controller.signal);
    // Keeps "5m ago" current.
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => {
      clearInterval(timer);
      controller.abort();
    };
  });
  createEffect(
    () => live.config.head?.id,
    (head) => {
      const known = revisions();
      if (head !== undefined && known !== undefined && known[0]?.id !== head) {
        load(controller.signal);
      }
    },
  );

  const loadOlder = () => {
    const last = revisions()?.at(-1);
    if (last === undefined || older() === "loading") return;
    setOlder("loading");
    api.revisions
      .page(last.id, PAGE, controller.signal)
      .then((page) => {
        setRevisions((known) => merge(known ?? [], page.revisions));
        setMore(page.more);
        setOlder("idle");
      })
      .catch(() => {
        if (!controller.signal.aborted) setOlder("failed");
      });
  };

  return (
    <Show
      when={revisions()}
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
      {(list) => (
        <Card class="overflow-hidden" aria-label={t.listTitle}>
          <div class="flex items-start gap-3 border-edge border-b px-4 py-2.5">
            <div class="min-w-0 flex-1">
              <h2 class="font-medium text-sm">{t.listTitle}</h2>
              <p role="status" class="text-fg-subtle text-xs tabular-nums">
                {t.count(list().length, more())}
              </p>
              <p class="mt-1 text-fg-muted text-xs">{t.hint}</p>
            </div>
            <Show when={props.admin}>
              <Button size="sm" icon={Download} onClick={() => navigate("/settings/import")}>
                {strings.settings.importLink}
              </Button>
            </Show>
          </div>
          <Show
            when={list().length > 0}
            fallback={<p class="px-6 py-12 text-center font-medium text-sm">{t.empty}</p>}
          >
            <ul>
              <For each={list()} keyed={(revision) => revision.id}>
                {(revision) => (
                  <RevisionRow
                    revision={revision()}
                    latest={revision().id === list()[0]?.id}
                    expanded={open() === revision().id}
                    onToggle={() =>
                      setOpen((now) => (now === revision().id ? undefined : revision().id))
                    }
                    revisions={list()}
                    now={now()}
                    nodeName={props.nodeName}
                    admin={props.admin}
                    latestId={list()[0]?.id ?? revision().id}
                  />
                )}
              </For>
            </ul>
          </Show>
          <Show when={more()}>
            <div class="flex flex-col items-center gap-2 border-edge/60 border-t px-4 py-3">
              <Show when={older() === "failed"}>
                <p role="alert" class="text-crit text-xs">
                  {t.moreFailed}
                </p>
              </Show>
              <Button size="sm" disabled={older() === "loading"} onClick={loadOlder}>
                {older() === "loading" ? t.loadingMore : t.more}
              </Button>
            </div>
          </Show>
        </Card>
      )}
    </Show>
  );
}
