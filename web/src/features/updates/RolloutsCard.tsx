import { Rocket } from "lucide";
import { createSignal, For, Show, useContext } from "solid-js";
import type { FirmwareUpload } from "../../api/gen/FirmwareUpload";
import type { Rollout } from "../../api/gen/Rollout";
import type { FirmwareRow } from "../../lib/firmware";
import { formatDateTime } from "../../lib/format";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { createRollouts, isActive } from "../../lib/rollouts";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { RequestError } from "../../ui/JobOutcome";
import { showToast } from "../../ui/Toast";
import { RolloutDialog } from "./RolloutDialog";
import { RolloutView, STATE_TONES } from "./RolloutView";

const t = strings.firmware.rollouts;

/**
 * Rolling upgrades on the Updates page (D-125): the active rollout's live pipeline (read again
 * every few seconds while it moves, its jobs from the `jobs` topic), the latest 20 below it, and
 * for admins *Roll out upgrades* while none is active. A rollout ending or halting is toasted.
 */
export function RolloutsCard(props: {
  admin: boolean;
  /** The devices an upgrade can be prepared for. */
  rows: readonly FirmwareRow[];
  uploads: readonly FirmwareUpload[];
}) {
  const live = useContext(LiveContext);
  const [jobs] = createJobsView(live);
  const { rollouts, error, put, active } = createRollouts({
    onChange: (_, after) => {
      if (after.state === "done") showToast({ title: t.done(after.id), tone: "ok" });
      if (after.state === "halted") {
        showToast({
          title: t.stopped(after.id),
          description: after.error ?? undefined,
          tone: "crit",
        });
      }
    },
  });
  const [starting, setStarting] = createSignal(false);
  const past = () => (rollouts() ?? []).filter((rollout) => !isActive(rollout.state));

  return (
    <Card class="mx-auto w-full max-w-7xl overflow-hidden" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin}>
            <Button
              size="sm"
              variant="primary"
              icon={Rocket}
              disabled={
                rollouts() === undefined || active() !== undefined || props.rows.length === 0
              }
              title={active() ? t.oneAtATime : undefined}
              onClick={() => setStarting(true)}
            >
              {t.start}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-4 px-4 pb-4">
        <RequestError text={error()} />
        <Show
          when={rollouts()}
          fallback={
            <Show when={!error()}>
              <p role="status" class="text-fg-subtle text-sm">
                {t.loading}
              </p>
            </Show>
          }
        >
          <Show when={active()}>
            {(current) => (
              <section aria-label={t.label(current().id)} class="flex flex-col gap-2">
                <h3 class="font-medium text-fg text-sm">{t.label(current().id)}</h3>
                <RolloutView
                  rollout={current()}
                  jobs={jobs}
                  uploads={props.uploads}
                  admin={props.admin}
                  onChange={put}
                />
              </section>
            )}
          </Show>
          <Show
            when={past().length > 0}
            fallback={
              <Show when={!active()}>
                <p class="text-fg-subtle text-sm">{t.empty}</p>
              </Show>
            }
          >
            <section aria-label={t.history} class="flex flex-col gap-2">
              <h3 class="font-medium text-fg text-sm">{t.history}</h3>
              <ul class="flex flex-col divide-y divide-edge rounded-control border border-edge">
                <For each={past()}>
                  {(rollout) => (
                    <PastRollout rollout={rollout} jobs={jobs} uploads={props.uploads} />
                  )}
                </For>
              </ul>
            </section>
          </Show>
        </Show>
      </div>
      <Show when={starting()}>
        <RolloutDialog
          open
          onOpenChange={setStarting}
          rows={props.rows}
          onStarted={(rollout) => {
            put(rollout);
          }}
        />
      </Show>
    </Card>
  );
}

/** A finished rollout: its summary, the pipeline as it ended a click away. */
function PastRollout(props: {
  rollout: Rollout;
  jobs: ReturnType<typeof createJobsView>[0];
  uploads: readonly FirmwareUpload[];
}) {
  const names = () => props.rollout.nodes.map((node) => node.name).join(", ");
  return (
    <li>
      <details class="group">
        <summary class="flex cursor-pointer flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-sm">
          <span class="font-medium">{t.label(props.rollout.id)}</span>
          <Badge tone={STATE_TONES[props.rollout.state]}>
            {t.state[props.rollout.state] ?? props.rollout.state}
          </Badge>
          <span class="min-w-0 truncate text-fg-muted text-xs" title={names()}>
            {t.devices(props.rollout.nodes.length)} · {names()}
          </span>
          <span class="ml-auto text-fg-subtle text-xs">
            {formatDateTime(props.rollout.started_at)}
          </span>
        </summary>
        <div class="px-3 pb-3">
          <RolloutView
            rollout={props.rollout}
            jobs={props.jobs}
            uploads={props.uploads}
            admin={false}
            onChange={() => {}}
          />
        </div>
      </details>
    </li>
  );
}
