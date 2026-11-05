import { Lock, RotateCw, Router, Undo2, Upload } from "lucide";
import { createEffect, createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { ApplyPlan } from "../../api/gen/ApplyPlan";
import type { Job } from "../../api/gen/Job";
import type { JobId } from "../../api/gen/JobId";
import { ApiError, api, errorText } from "../../lib/api";
import { lastApplied } from "../../lib/config";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Dialog } from "../../ui/Dialog";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { showToast } from "../../ui/Toast";
import { ApplyProgress } from "./ApplyProgress";
import { ImpactCard } from "./ImpactCard";
import { NodePlanCard } from "./NodePlanCard";
import { counts, split } from "./plan";

const t = strings.changes;

/**
 * `/changes` (features/lifecycle.md, Render → diff → apply; D-071): what applying the latest
 * revision does to each managed node, read from them just now (the impact, each node's diffs
 * with secrets masked, the nodes refused or left alone), then the apply followed live on the
 * `jobs` topic, and discarding the changes back to what the nodes hold. Admins only.
 */
export function ChangesPage() {
  const session = useContext(SessionContext);
  return (
    <div class="mx-auto flex max-w-5xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
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
        <Review />
      </Show>
    </div>
  );
}

function Review() {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [plan, setPlan] = createSignal<ApplyPlan>();
  const [planError, setPlanError] = createSignal<string>();
  const [loading, setLoading] = createSignal(false);
  const [applying, setApplying] = createSignal(false);
  /** The apply shown: the one started here, or one found running; kept once it ends. */
  const [shown, setShown] = createSignal<JobId>();
  const [discarding, setDiscarding] = createSignal(false);
  const [asking, setAsking] = createSignal(false);

  let controller: AbortController | undefined;
  /** Plans again, keeping the plan shown until the new one arrives. */
  const load = () => {
    controller?.abort();
    const own = new AbortController();
    controller = own;
    setLoading(true);
    setPlanError(undefined);
    api.apply
      .plan(own.signal)
      .then((next) => {
        if (!Array.isArray(next?.nodes)) throw new Error("not an apply plan");
        if (!own.signal.aborted) setPlan(next);
      })
      .catch((err) => {
        if (!own.signal.aborted) setPlanError(errorText(err) ?? t.failed);
      })
      .finally(() => {
        if (controller === own) setLoading(false);
      });
  };
  onSettled(() => {
    load();
    return () => controller?.abort();
  });

  const running = createMemo(() =>
    Object.values(jobs.byId).find((job) => job.kind === "apply" && job.state === "running"),
  );
  // An apply started elsewhere stays shown once it ends, until "Review again".
  createEffect(running, (found) => {
    if (found && shown() === undefined) setShown(found.id);
  });
  const job = createMemo((): Job | undefined => {
    const id = shown();
    return (id === undefined ? undefined : jobs.byId[id]) ?? running();
  });

  const head = () => live.config.head;
  const stale = () => {
    const reviewed = plan()?.revision;
    const latest = head()?.id;
    return reviewed !== undefined && latest !== undefined && latest !== reviewed;
  };
  const back = () => lastApplied(live.config, live.fleet.nodes);
  const tally = createMemo(() => {
    const current = plan();
    return current ? counts(current) : undefined;
  });
  const managed = () => {
    const c = tally();
    return c ? c.change + c.unchanged + c.refused : 0;
  };
  const canApply = () =>
    !stale() && !loading() && !applying() && (tally()?.refused ?? 0) === 0 && managed() > 0;

  const apply = async () => {
    const current = plan();
    if (!current) return;
    setApplying(true);
    try {
      const started = await api.apply.start(current.revision);
      addJobs([started]);
      setShown(started.id);
    } catch (err) {
      showToast({ title: t.applyFailed, description: errorText(err), tone: "crit" });
      if (err instanceof ApiError && err.status === 409) load();
    } finally {
      setApplying(false);
    }
  };

  const discard = async () => {
    const to = back();
    const base = head()?.id;
    if (to === undefined || base === undefined) return;
    setDiscarding(true);
    try {
      await api.revisions.restore(to, { base, message: t.discard.message(to) });
      setAsking(false);
      showToast({ title: t.discard.done(to), tone: "ok" });
      load();
    } catch (err) {
      showToast({ title: t.discard.failed, description: errorText(err), tone: "crit" });
    } finally {
      setDiscarding(false);
    }
  };

  const again = () => {
    setShown(undefined);
    load();
  };

  return (
    <>
      <Show
        when={job()}
        fallback={
          <Show
            when={plan()}
            fallback={
              <Card>
                <Show
                  when={planError()}
                  fallback={
                    <div
                      role="status"
                      class="flex flex-col items-center gap-2 px-6 py-16 text-center"
                    >
                      <Icon icon={RotateCw} class="size-5 animate-spin text-accent" />
                      <p class="font-medium text-sm">{t.loading}…</p>
                      <p class="max-w-md text-fg-muted text-sm">{t.loadingHint}</p>
                    </div>
                  }
                >
                  {(message) => (
                    <div
                      role="alert"
                      class="flex flex-col items-center gap-3 px-6 py-14 text-center"
                    >
                      <p class="font-medium text-sm">{t.failed}</p>
                      <p class="max-w-lg break-words text-fg-muted text-sm">{message()}</p>
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
                <Card aria-label={t.title}>
                  <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-start">
                    <div class="min-w-0 flex-1">
                      <p class="font-medium text-fg">
                        {t.revision(
                          current().revision,
                          head()?.id === current().revision
                            ? head()?.message || strings.settings.revisions.untitled
                            : strings.settings.revisions.number(current().revision),
                        )}
                      </p>
                      <div class="mt-2 flex flex-wrap gap-1.5" role="status">
                        <Show when={tally()?.change}>
                          {(n) => <Badge tone="info">{t.count.change(n())}</Badge>}
                        </Show>
                        <Show when={tally()?.unchanged}>
                          {(n) => <Badge tone="ok">{t.count.unchanged(n())}</Badge>}
                        </Show>
                        <Show when={tally()?.skipped}>
                          {(n) => <Badge>{t.count.skipped(n())}</Badge>}
                        </Show>
                        <Show when={tally()?.refused}>
                          {(n) => <Badge tone="crit">{t.count.refused(n())}</Badge>}
                        </Show>
                      </div>
                      <Show when={managed() > 0}>
                        <p class="mt-2.5 text-fg-muted text-sm">
                          {tally()?.change ? t.applyHint : t.nothing(current().revision)}
                        </p>
                      </Show>
                    </div>
                    <div class="flex shrink-0 flex-wrap gap-2">
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={RotateCw}
                        disabled={loading()}
                        onClick={load}
                      >
                        {t.again}
                      </Button>
                      <Show when={back()}>
                        <Button
                          variant="secondary"
                          size="sm"
                          icon={Undo2}
                          onClick={() => setAsking(true)}
                        >
                          {t.discard.action}
                        </Button>
                      </Show>
                      <Show when={managed() > 0}>
                        <Button
                          variant="primary"
                          size="sm"
                          icon={Upload}
                          disabled={!canApply()}
                          onClick={apply}
                        >
                          {applying() ? t.applying : t.apply(current().revision)}
                        </Button>
                      </Show>
                    </div>
                  </div>
                  <Show when={stale() && head()}>
                    {(latest) => (
                      <p
                        role="alert"
                        class="border-warn/40 border-t bg-warn/10 px-4 py-2.5 text-fg text-sm"
                      >
                        {t.stale(latest().id)}
                      </p>
                    )}
                  </Show>
                  <Show when={planError()}>
                    <div class="px-4 pb-4">
                      <RequestError text={`${t.failed}: ${planError()}`} />
                    </div>
                  </Show>
                </Card>
                <Show when={managed() === 0}>
                  <Card>
                    <EmptyState icon={Router} {...t.noneManaged}>
                      <a
                        href="/devices"
                        class="mt-4 font-medium text-accent text-sm hover:underline"
                      >
                        {t.noneManaged.action}
                      </a>
                    </EmptyState>
                  </Card>
                </Show>
                <Show when={tally()?.refused}>
                  <div
                    role="alert"
                    class="rounded-card border border-crit/40 bg-crit/10 px-4 py-3 text-sm"
                  >
                    <p class="font-medium text-fg">{t.refused.title}</p>
                    <p class="mt-0.5 text-fg-muted">{t.refused.hint}</p>
                  </div>
                </Show>
                <ImpactCard plan={current()} />
                <Nodes plan={current()} />
              </>
            )}
          </Show>
        }
      >
        {(running) => <ApplyProgress job={running()} onAgain={again} />}
      </Show>
      <Dialog
        open={asking()}
        onOpenChange={setAsking}
        title={t.discard.title(back() ?? 0)}
        description={t.discard.body(back() ?? 0)}
      >
        <div class="flex justify-end gap-2 px-5 pt-2 pb-5">
          <Button variant="ghost" onClick={() => setAsking(false)}>
            {t.discard.cancel}
          </Button>
          <Button variant="primary" disabled={discarding()} onClick={discard}>
            {t.discard.confirm}
          </Button>
        </div>
      </Dialog>
    </>
  );
}

function Nodes(props: { plan: ApplyPlan }) {
  const parts = () => split(props.plan);
  return (
    <>
      <Show when={parts().acted.length > 0}>
        <section class="flex flex-col gap-3" aria-label={t.order}>
          <h2 class="mt-2 font-medium text-fg-muted text-sm">{t.order}</h2>
          <For each={parts().acted}>{(node) => <NodePlanCard plan={node} />}</For>
        </section>
      </Show>
      <Show when={parts().alone.length > 0}>
        <section class="flex flex-col gap-3" aria-label={t.leftAlone}>
          <h2 class="mt-2 font-medium text-fg-muted text-sm">{t.leftAlone}</h2>
          <For each={parts().alone}>{(node) => <NodePlanCard plan={node} />}</For>
        </section>
      </Show>
    </>
  );
}
