import { createEffect, createSignal, For, Match, Show, Switch } from "solid-js";
import type { ApplyPlan } from "../../api/gen/ApplyPlan";
import type { Job } from "../../api/gen/Job";
import type { JobId } from "../../api/gen/JobId";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { SiteView } from "../../api/gen/SiteView";
import { ApplyProgress } from "../../features/changes/ApplyProgress";
import { ImpactCard } from "../../features/changes/ImpactCard";
import { NodePlanCard } from "../../features/changes/NodePlanCard";
import { ApiError, api, errorText } from "../../lib/api";
import type { JobsState } from "../../lib/jobs";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { RequestError } from "../../ui/JobOutcome";
import { Note } from "./RouterStep";

const t = strings.setup.review;

/**
 * The wizard's last step (ui.md, Interaction patterns: every wizard ends in a review): the
 * choices in words; then the plan saved as one revision and checked with the router (what
 * restarts, what it refuses); then the apply, live, with the bridge, Settle and WAN steps of
 * a renumbering (D-081).
 */
export function ReviewStep(props: {
  gateway: NodeSummary;
  /** The choices, as `[label, value]`. */
  rows: readonly (readonly [string, string])[];
  /** The router's address changes, and this controller's with it. */
  renumbers: boolean;
  /** Saves the choices as the site's next revision. */
  save: () => Promise<SiteView>;
  jobs: JobsState;
  addJobs: (jobs: readonly Job[]) => void;
  onBack: () => void;
  onInternet: () => void;
  /** The apply succeeded. */
  onDone: () => void;
  onLeave: (to: string) => void;
}) {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [issues, setIssues] = createSignal<string[]>([]);
  const [plan, setPlan] = createSignal<ApplyPlan>();
  const [jobId, setJobId] = createSignal<JobId>();
  const [done, setDone] = createSignal(false);

  const job = () => {
    const id = jobId();
    return id === undefined ? undefined : props.jobs.byId[id];
  };
  const refused = () => plan()?.nodes.filter((node) => node.action === "refused") ?? [];
  const changing = () => plan()?.nodes.filter((node) => node.action === "change") ?? [];
  const warnings = () => changing().flatMap((node) => node.warnings.map((w) => w.message));

  const check = async () => {
    setBusy(true);
    setError(undefined);
    setIssues([]);
    setPlan(undefined);
    setJobId(undefined);
    try {
      await props.save();
      setPlan(await api.apply.plan());
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        setIssues(err.issues.map((issue) => issue.message));
      }
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    const reviewed = plan();
    if (!reviewed) return;
    setBusy(true);
    setError(undefined);
    try {
      const started = await api.apply.start(reviewed.revision);
      props.addJobs([started]);
      setJobId(started.id);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  createEffect(
    () => job()?.state,
    (state) => {
      if (state !== "succeeded") return;
      setDone(true);
      props.onDone();
    },
  );
  const wanFailed = () => job()?.error?.code === "partial_apply";

  return (
    <div class="flex flex-col gap-5">
      <div>
        <h2 class="font-semibold text-lg tracking-tight">{t.heading}</h2>
        <Show when={!job()}>
          <p class="mt-1.5 text-fg-muted text-sm">{t.intro}</p>
        </Show>
      </div>

      <Show when={!job()}>
        <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-control border border-edge px-4 py-3 text-sm">
          <For each={props.rows}>
            {(row) => (
              <>
                <dt class="text-fg-muted">{row[0]}</dt>
                <dd class="min-w-0 break-words">{row[1]}</dd>
              </>
            )}
          </For>
        </dl>
        <Show when={props.renumbers}>
          <Note tone="warn" title={t.movesTitle} text={t.movesHint} />
        </Show>
      </Show>

      <Switch>
        <Match when={job()}>
          {(running) => (
            <>
              <Show when={done()}>
                <Note
                  tone="ok"
                  title={t.done}
                  text={t.doneHint(props.gateway.name, props.gateway.host)}
                />
              </Show>
              <Show when={wanFailed()}>
                <Note tone="warn" title={t.doneWan} />
              </Show>
              <ApplyProgress job={running()} onAgain={() => void check()} />
            </>
          )}
        </Match>
        <Match when={busy() && !plan()}>
          <p class="text-fg-muted text-sm" role="status">
            {t.checking}
          </p>
        </Match>
        <Match when={plan()}>
          {(reviewed) => (
            <div class="flex flex-col gap-4">
              <p class="text-fg-muted text-sm">{t.checked(reviewed().revision)}</p>
              <Show
                when={refused().length > 0}
                fallback={
                  <>
                    <ImpactCard plan={reviewed()} />
                    <Show when={warnings().length > 0}>
                      <Note tone="warn" title={t.warnings}>
                        <ul class="mt-1 list-disc pl-4 text-fg-muted">
                          <For each={warnings()}>{(warning) => <li>{warning}</li>}</For>
                        </ul>
                      </Note>
                    </Show>
                    <details class="rounded-control border border-edge">
                      <summary class="cursor-pointer select-none px-3 py-2 font-medium text-sm">
                        {t.details}
                      </summary>
                      <div class="flex flex-col gap-3 border-edge border-t p-3">
                        <For each={changing()}>{(node) => <NodePlanCard plan={node} />}</For>
                      </div>
                    </details>
                  </>
                }
              >
                <Note tone="warn" title={t.refused} />
                <For each={refused()}>{(node) => <NodePlanCard plan={node} />}</For>
              </Show>
            </div>
          )}
        </Match>
      </Switch>

      <Show when={issues().length > 0}>
        <ul class="list-disc pl-5 text-crit text-sm">
          <For each={issues()}>{(issue) => <li>{issue}</li>}</For>
        </ul>
      </Show>
      <RequestError text={error()} />

      <div class="flex flex-wrap justify-end gap-2">
        <Switch>
          <Match when={done()}>
            <Button variant="secondary" onClick={() => props.onLeave("/devices")}>
              {t.devices}
            </Button>
            <Button variant="primary" onClick={() => props.onLeave("/")}>
              {t.dashboard}
            </Button>
          </Match>
          <Match when={job()?.state === "running"}>
            <Button disabled>{t.applying}</Button>
          </Match>
          <Match when={job()}>
            <Button variant="ghost" onClick={() => setJobId(undefined)}>
              {strings.setup.back}
            </Button>
            <Show when={wanFailed()}>
              <Button variant="secondary" onClick={props.onInternet}>
                {t.toInternet}
              </Button>
            </Show>
            <Button variant="primary" onClick={() => void check()}>
              {t.again}
            </Button>
          </Match>
          <Match when={true}>
            <Button variant="ghost" disabled={busy()} onClick={props.onBack}>
              {strings.setup.back}
            </Button>
            <Show
              when={plan() && refused().length === 0}
              fallback={
                <Button variant="primary" disabled={busy()} onClick={() => void check()}>
                  {plan() ? t.again : t.check}
                </Button>
              }
            >
              <Button variant="primary" disabled={busy()} onClick={() => void apply()}>
                {t.apply}
              </Button>
            </Show>
          </Match>
        </Switch>
      </div>
    </div>
  );
}
