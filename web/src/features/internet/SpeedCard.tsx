import { Activity, Gauge, Lightbulb } from "lucide";
import { createEffect, createSignal, For, Show, useContext } from "solid-js";
import type { JobId } from "../../api/gen/JobId";
import type { SpeedTest } from "../../api/gen/SpeedTest";
import type { SpeedTests } from "../../api/gen/SpeedTests";
import type { Suggestion } from "../../api/gen/Suggestion";
import { api, errorText } from "../../lib/api";
import { formatDateTime, formatRate } from "../../lib/format";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { JobErrorNote, RequestError } from "../../ui/JobOutcome";
import { JobSteps } from "../../ui/JobSteps";
import { gradeTone } from "./uplink";

const t = strings.internet.speed;

/** The grade's box, in its tone. */
const GRADE_BOX: Record<Tone, string> = {
  ok: "border-ok/40 bg-ok/10 text-ok",
  warn: "border-warn/40 bg-warn/10 text-warn",
  crit: "border-crit/40 bg-crit/10 text-crit",
  info: "border-info/40 bg-info/10 text-info",
  neutral: "border-edge",
};

const ms = (us: number) => `${(us / 1000).toFixed(1)} ms`;

/** The newest test's numbers, its grade and what the grade means. */
function Result(props: { test: SpeedTest }) {
  const tiles = () => [
    [t.down, formatRate(props.test.download_kbps)],
    [t.up, formatRate(props.test.upload_kbps)],
    [t.idle, ms(props.test.idle_us)],
    [t.loadedDown, ms(props.test.download_us)],
    [t.loadedUp, ms(props.test.upload_us)],
  ];
  return (
    <div class="flex flex-col gap-3">
      <div class="flex flex-wrap items-center gap-3">
        <div
          class={[
            "flex size-12 items-center justify-center rounded-card border font-semibold text-xl tabular-nums",
            GRADE_BOX[gradeTone(props.test.grade)],
          ]}
          role="img"
          aria-label={`${t.grade} ${props.test.grade}`}
        >
          {props.test.grade}
        </div>
        <div class="min-w-0 flex-1 text-sm">
          <p class="flex items-center gap-2">
            <Badge tone={gradeTone(props.test.grade)}>{t.grade}</Badge>
            <span class="text-fg-muted text-xs">
              {props.test.bypassed ? t.bypassed : t.shaped} · {formatDateTime(props.test.at)}
            </span>
          </p>
          <p class="mt-1 text-fg-muted">{t.grades[props.test.grade]}</p>
        </div>
      </div>
      <dl class="grid grid-cols-2 gap-2 sm:grid-cols-5">
        <For each={tiles()}>
          {([label, value]) => (
            <div class="rounded-control border border-edge px-3 py-2">
              <dt class="text-fg-subtle text-xs">{label}</dt>
              <dd class="font-medium tabular-nums">{value}</dd>
            </div>
          )}
        </For>
      </dl>
    </div>
  );
}

/** What the newest test of the line suggests, and the way to take it. */
function SuggestionNote(props: { suggestion: Suggestion; admin: boolean; onTake: () => void }) {
  return (
    <div class="flex flex-wrap items-center gap-3 rounded-control border border-info/40 bg-info/10 px-3 py-2.5 text-sm">
      <Icon icon={Lightbulb} class="size-4 shrink-0 text-info" />
      <div class="min-w-0 flex-1">
        <Show when={props.suggestion.drifted}>
          <p class="font-medium">{t.drifted}</p>
        </Show>
        <p>
          {t.suggestion(
            formatRate(props.suggestion.download_kbps),
            formatRate(props.suggestion.upload_kbps),
          )}
        </p>
      </div>
      <Show when={props.admin}>
        <Button size="sm" onClick={() => props.onTake()}>
          {t.use}
        </Button>
      </Show>
    </div>
  );
}

/**
 * Speed tests on the Internet page (wan.md, SQM; D-076): the newest result graded for
 * bufferbloat, the suggestion of the newest test of the line, the history, and for admins the
 * tests themselves, followed live.
 */
export function SpeedCard(props: {
  tests: SpeedTests | undefined;
  admin: boolean;
  /** Stopping SQM needs management on the gateway. */
  managed: boolean;
  sqmOn: boolean;
  onTake: (suggestion: Suggestion) => void;
  onDone: () => void;
}) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [jobId, setJobId] = createSignal<JobId>();
  const [failure, setFailure] = createSignal<string>();
  const job = () => {
    const id = jobId();
    return id === undefined ? undefined : jobs.byId[id];
  };
  const running = () => job()?.state === "running";

  createEffect(
    () => job()?.state,
    (state) => {
      if (state === "succeeded" || state === "failed") props.onDone();
    },
  );

  const run = async (bypass_sqm: boolean) => {
    setFailure(undefined);
    try {
      const started = await api.wan.speedTest({ bypass_sqm });
      addJobs([started]);
      setJobId(started.id);
    } catch (err) {
      setFailure(errorText(err));
    }
  };
  const latest = () => props.tests?.tests[0];

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin}>
            <div class="flex gap-2">
              <Show when={props.managed}>
                <Button
                  size="sm"
                  icon={Gauge}
                  title={t.measureHint}
                  disabled={running()}
                  onClick={() => void run(true)}
                >
                  {t.measure}
                </Button>
              </Show>
              <Button
                size="sm"
                variant="primary"
                icon={Activity}
                disabled={running()}
                onClick={() => void run(false)}
              >
                {t.run}
              </Button>
            </div>
          </Show>
        }
      />
      <div class="flex flex-col gap-4 px-4 pb-4">
        <Show when={job()}>
          {(current) => (
            <div class="flex flex-col gap-3">
              <div class="rounded-control border border-edge p-3" role="status" aria-live="polite">
                <JobSteps job={current()} />
              </div>
              <Show when={current().state === "failed" && current().error}>
                {(error) => <JobErrorNote error={error()} />}
              </Show>
            </div>
          )}
        </Show>
        <RequestError text={failure()} />
        <Show
          when={latest()}
          fallback={<p class="text-fg-muted text-sm">{props.tests ? t.none : ""}</p>}
        >
          {(test) => <Result test={test()} />}
        </Show>
        <Show when={props.tests?.suggestion}>
          {(suggestion) => (
            <Show when={!props.sqmOn || suggestion().drifted}>
              <SuggestionNote
                suggestion={suggestion()}
                admin={props.admin}
                onTake={() => props.onTake(suggestion())}
              />
            </Show>
          )}
        </Show>
        <Show when={(props.tests?.tests.length ?? 0) > 1}>
          <div>
            <h3 class="mb-1.5 font-medium text-fg-muted text-xs">{t.history}</h3>
            <table class="w-full text-sm">
              <thead class="text-left text-fg-subtle text-xs">
                <tr>
                  <th class="py-1 font-normal">{t.when}</th>
                  <th class="py-1 font-normal">{t.kind}</th>
                  <th class="py-1 text-right font-normal">{t.down}</th>
                  <th class="py-1 text-right font-normal">{t.up}</th>
                  <th class="py-1 text-right font-normal">{t.grade}</th>
                </tr>
              </thead>
              <tbody class="tabular-nums">
                <For each={props.tests?.tests.slice(0, 10) ?? []}>
                  {(test) => (
                    <tr class="border-edge border-t">
                      <td class="py-1">{formatDateTime(test.at)}</td>
                      <td class="py-1 text-fg-muted">{test.bypassed ? t.bypassed : t.shaped}</td>
                      <td class="py-1 text-right">{formatRate(test.download_kbps)}</td>
                      <td class="py-1 text-right">{formatRate(test.upload_kbps)}</td>
                      <td class="py-1 text-right">
                        <Badge tone={gradeTone(test.grade)}>{test.grade}</Badge>
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </div>
    </Card>
  );
}
