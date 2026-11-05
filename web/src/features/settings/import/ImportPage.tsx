import { useNavigate } from "@solidjs/router";
import { CircleCheck, Download, Lock, RotateCw } from "lucide";
import { createEffect, createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { ImportPreview } from "../../../api/gen/ImportPreview";
import { ApiError, api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { EmptyState } from "../../../ui/EmptyState";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import { showToast } from "../../../ui/Toast";
import { ChangeList } from "../revisions/ChangeList";
import { NodeImportCard } from "./NodeImportCard";
import { sortIssues, stateCounts } from "./preview";

const t = strings.settings.import;

/**
 * `/settings/import`, Settings › Import (lifecycle.md, Brownfield import; D-073): reads every
 * adopted node into the latest revision as soon as it opens, shows what the site would become
 * and, per node, how much the model holds, its sections' fates and the residual; accepting saves
 * exactly the preview reviewed as the next revision. Admins only.
 */
export function ImportPage() {
  const session = useContext(SessionContext);
  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );
  return (
    <div class="mx-auto flex max-w-5xl flex-col gap-4">
      <header>
        <nav aria-label={strings.nav.settings.label} class="text-fg-subtle text-xs">
          <a href="/settings?tab=revisions" class="hover:text-fg hover:underline">
            {strings.nav.settings.label}
          </a>
          <span aria-hidden="true">{" › "}</span>
          <span aria-current="page">{t.crumb}</span>
        </nav>
        <h1 class="mt-1 font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{t.description}</p>
      </header>
      <Show
        when={session.user.role === "admin"}
        fallback={
          <Card>
            <EmptyState icon={Lock} {...t.adminOnly} />
          </Card>
        }
      >
        <Flow />
      </Show>
    </div>
  );
}

function Flow() {
  const live = useContext(LiveContext);
  const navigate = useNavigate();
  const [preview, setPreview] = createSignal<ImportPreview>();
  const [error, setError] = createSignal<string>();
  const [loading, setLoading] = createSignal(false);
  const [message, setMessage] = createSignal("");
  const [accepting, setAccepting] = createSignal(false);
  /** The revision the accepted import saved; the base when it saved nothing. */
  const [saved, setSaved] = createSignal<{ id: number; base: number }>();

  let controller: AbortController | undefined;
  /** Reads the nodes again, keeping the preview shown until the new one arrives. */
  const load = () => {
    controller?.abort();
    const own = new AbortController();
    controller = own;
    setLoading(true);
    setError(undefined);
    api.import
      .preview(own.signal)
      .then((next) => {
        if (!Array.isArray(next?.nodes)) throw new Error("not an import preview");
        if (!own.signal.aborted) {
          setPreview(next);
          setSaved(undefined);
        }
      })
      .catch((err) => {
        if (!own.signal.aborted) setError(errorText(err) ?? t.failed);
      })
      .finally(() => {
        if (controller === own) setLoading(false);
      });
  };
  onSettled(() => {
    load();
    return () => controller?.abort();
  });

  const sorted = createMemo(() => sortIssues(preview()?.issues ?? []));
  const tally = createMemo(() => {
    const current = preview();
    return current ? stateCounts(current) : undefined;
  });
  const stale = () => {
    const base = preview()?.base;
    const head = live.config.head?.id;
    return base !== undefined && head !== undefined && head !== base && saved() === undefined;
  };
  const changes = () => preview()?.changes.length ?? 0;
  const canAccept = () =>
    !stale() && !loading() && !accepting() && sorted().errors.length === 0 && changes() > 0;

  const accept = async () => {
    const current = preview();
    if (!current) return;
    setAccepting(true);
    try {
      const text = message().trim();
      const view = await api.import.accept(current.id, text === "" ? null : text);
      setSaved({ id: view.revision.id, base: current.base });
      setMessage("");
    } catch (err) {
      showToast({ title: t.acceptFailed, description: errorText(err), tone: "crit" });
      // Expired, or another revision was saved since: only a new preview can be accepted.
      if (err instanceof ApiError && (err.status === 404 || err.status === 409)) load();
    } finally {
      setAccepting(false);
    }
  };

  return (
    <Show
      when={preview()}
      fallback={
        <Card>
          <Show
            when={error()}
            fallback={
              <div role="status" class="flex flex-col items-center gap-2 px-6 py-16 text-center">
                <Icon icon={RotateCw} class="size-5 animate-spin text-accent" />
                <p class="font-medium text-sm">{t.loading}…</p>
                <p class="max-w-md text-fg-muted text-sm">{t.loadingHint}</p>
              </div>
            }
          >
            {(text) => (
              <div role="alert" class="flex flex-col items-center gap-3 px-6 py-14 text-center">
                <p class="font-medium text-sm">{t.failed}</p>
                <p class="max-w-lg break-words text-fg-muted text-sm">{text()}</p>
                <Button size="sm" icon={RotateCw} onClick={load}>
                  {t.retry}
                </Button>
              </div>
            )}
          </Show>
        </Card>
      }
    >
      {(current) => (
        <>
          <Show
            when={saved()}
            fallback={
              <Card aria-label={t.crumb}>
                <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-start">
                  <div class="min-w-0 flex-1">
                    <p class="font-medium text-fg">{t.into(current().base)}</p>
                    <div class="mt-2 flex flex-wrap gap-1.5" role="status">
                      <Show when={tally()?.imported}>
                        {(n) => <Badge tone="ok">{t.count.imported(n())}</Badge>}
                      </Show>
                      <Show when={tally()?.unread}>
                        {(n) => <Badge tone="warn">{t.count.unread(n())}</Badge>}
                      </Show>
                      <Show when={tally()?.failed}>
                        {(n) => <Badge tone="crit">{t.count.failed(n())}</Badge>}
                      </Show>
                      <Badge tone={changes() > 0 ? "info" : "neutral"}>
                        {t.count.changes(changes())}
                      </Badge>
                    </div>
                    <p class="mt-2.5 text-fg-muted text-sm">
                      {sorted().errors.length > 0
                        ? t.blocked
                        : changes() > 0
                          ? t.ready(current().base + 1)
                          : t.nothing}
                    </p>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={RotateCw}
                    disabled={loading()}
                    onClick={load}
                  >
                    {loading() ? `${t.loading}…` : t.again}
                  </Button>
                </div>
                <Show when={stale() && live.config.head}>
                  {(head) => (
                    <p
                      role="alert"
                      class="border-warn/40 border-t bg-warn/10 px-4 py-2.5 text-fg text-sm"
                    >
                      {t.stale(head().id)}
                    </p>
                  )}
                </Show>
                <Show when={error()}>
                  <div class="px-4 pb-4">
                    <RequestError text={`${t.failed}: ${error()}`} />
                  </div>
                </Show>
                <Show when={sorted().errors.length > 0}>
                  <Issues title={t.errors} tone="crit" issues={sorted().errors} />
                </Show>
                <Show when={sorted().warnings.length > 0}>
                  <Issues title={t.warnings} tone="warn" issues={sorted().warnings} />
                </Show>
                <Show when={changes() > 0 && sorted().errors.length === 0}>
                  <form
                    class="flex flex-col gap-3 border-edge/60 border-t px-4 py-4 sm:flex-row sm:items-end"
                    onSubmit={(event) => {
                      event.preventDefault();
                      if (canAccept()) accept();
                    }}
                  >
                    <div class="min-w-0 flex-1">
                      <TextField
                        label={t.message}
                        value={message()}
                        onInput={setMessage}
                        hint={t.messageHint}
                        disabled={accepting()}
                      />
                    </div>
                    <Button type="submit" variant="primary" icon={Download} disabled={!canAccept()}>
                      {accepting() ? t.accepting : t.accept(current().base + 1)}
                    </Button>
                  </form>
                </Show>
              </Card>
            }
          >
            {(done) => (
              <Card aria-label={t.crumb}>
                <div role="status" class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-start">
                  <Icon icon={CircleCheck} class="mt-0.5 size-5 shrink-0 text-ok" />
                  <div class="min-w-0 flex-1">
                    <p class="font-medium text-fg">
                      {done().id === done().base ? t.done.none : t.done.title(done().id)}
                    </p>
                    <Show when={done().id !== done().base}>
                      <p class="mt-1 text-fg-muted text-sm">{t.done.body}</p>
                    </Show>
                  </div>
                  <div class="flex shrink-0 flex-wrap gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => navigate("/settings?tab=revisions")}
                    >
                      {t.done.revisions}
                    </Button>
                    <Button variant="primary" size="sm" onClick={() => navigate("/changes")}>
                      {t.done.review}
                    </Button>
                  </div>
                </div>
              </Card>
            )}
          </Show>
          <Card aria-label={t.changesTitle}>
            <CardHeader
              title={t.changesTitle}
              description={t.changesHint}
              actions={<span class="tabular-nums">{t.count.changes(changes())}</span>}
            />
            <div class="px-4 pb-4">
              <Show
                when={changes() > 0}
                fallback={<p class="text-fg-muted text-sm">{t.noChanges}</p>}
              >
                <ChangeList
                  changes={current().changes}
                  nodeName={(id) => current().nodes.find((n) => n.node === id)?.name}
                />
              </Show>
            </div>
          </Card>
          <section class="flex flex-col gap-3" aria-label={t.devices}>
            <h2 class="mt-2 font-medium text-fg-muted text-sm">{t.devices}</h2>
            <For each={current().nodes}>
              {(node) => <NodeImportCard node={node} site={sorted().byNode.get(node.node) ?? []} />}
            </For>
          </section>
        </>
      )}
    </Show>
  );
}

function Issues(props: {
  title: string;
  tone: "crit" | "warn";
  issues: readonly { path: string; message: string }[];
}) {
  return (
    <div
      role={props.tone === "crit" ? "alert" : undefined}
      class={[
        "border-t px-4 py-3 text-sm",
        props.tone === "crit" ? "border-crit/40 bg-crit/10" : "border-warn/40 bg-warn/10",
      ]}
    >
      <p class="font-medium text-fg">{props.title}</p>
      <ul class="mt-1 flex flex-col gap-0.5 text-fg-muted">
        <For each={props.issues}>
          {(issue) => (
            <li>
              <span class="font-mono text-fg-subtle text-xs">{issue.path}</span> {issue.message}
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}
