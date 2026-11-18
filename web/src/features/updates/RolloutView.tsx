import { Pause, Play, SkipForward, X } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { FirmwareUpload } from "../../api/gen/FirmwareUpload";
import type { Rollout } from "../../api/gen/Rollout";
import type { RolloutNode } from "../../api/gen/RolloutNode";
import type { RolloutNodeState } from "../../api/gen/RolloutNodeState";
import type { RolloutState } from "../../api/gen/RolloutState";
import type { RolloutWait } from "../../api/gen/RolloutWait";
import { api, errorText } from "../../lib/api";
import { clockText, formatDateTime, formatDuration } from "../../lib/format";
import type { JobsState } from "../../lib/jobs";
import { choiceShort } from "../../lib/rollouts";
import { strings } from "../../lib/strings";
import { Badge, StatusDot, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { JobErrorNote, RequestError } from "../../ui/JobOutcome";
import { JobSteps } from "../../ui/JobSteps";

const t = strings.firmware.rollouts;

export const STATE_TONES: Record<RolloutState, Tone> = {
  running: "info",
  paused: "warn",
  halted: "crit",
  done: "ok",
  cancelled: "neutral",
};
const NODE_TONES: Record<RolloutNodeState, Tone> = {
  pending: "neutral",
  preparing: "info",
  upgrading: "info",
  upgraded: "ok",
  failed: "crit",
  skipped: "neutral",
};

/** An action that asks first, and what it asks. */
type Asked =
  | { kind: "resume"; name: string | null }
  | { kind: "cancel" }
  | { kind: "skip"; node: number; name: string };

/**
 * A rollout's pipeline (D-125): what it waits for, why it halted, each device in the order the
 * controller runs them with its state and, while it upgrades (or when it failed), its job's steps
 * from the `jobs` topic. With `admin`, the actions its state allows: pause; resume, skip the
 * failed device and cancel, each confirmed first.
 */
export function RolloutView(props: {
  rollout: Rollout;
  jobs: JobsState;
  uploads: readonly FirmwareUpload[];
  admin: boolean;
  onChange: (rollout: Rollout) => void;
}) {
  const [asked, setAsked] = createSignal<Asked>();
  const [sending, setSending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const name = (node: number) =>
    props.rollout.nodes.find((entry) => entry.node === node)?.name ?? `#${node}`;
  const failed = () => props.rollout.nodes.find((entry) => entry.state === "failed");
  const upgrading = () =>
    props.rollout.nodes.some((entry) => entry.state === "upgrading" || entry.state === "preparing");
  /** The halt came after the canary's soak: the canary itself upgraded but its check failed. */
  const canaryHalt = () => props.rollout.state === "halted" && failed() === undefined;
  const isActive = () =>
    props.rollout.state === "running" ||
    props.rollout.state === "paused" ||
    props.rollout.state === "halted";

  const send = async (request: () => Promise<Rollout>) => {
    setSending(true);
    setError(undefined);
    try {
      props.onChange(await request());
      setAsked(undefined);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSending(false);
    }
  };
  const run = (action: Asked | { kind: "pause" }) => {
    const id = props.rollout.id;
    const rollouts = api.firmware.rollouts;
    if (action.kind === "skip") return send(() => rollouts.skip(id, action.node));
    return send(() => rollouts.act(id, action.kind));
  };
  const question = (action: Asked) => {
    switch (action.kind) {
      case "resume":
        return t.confirm.resume(action.name);
      case "skip":
        return t.confirm.skip(action.name);
      case "cancel":
        return t.confirm.cancel;
    }
  };

  return (
    <div class="flex flex-col gap-3">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
        <Badge tone={STATE_TONES[props.rollout.state]}>
          {t.state[props.rollout.state] ?? props.rollout.state}
        </Badge>
        <span class="text-fg-muted text-xs">
          {t.started(formatDateTime(props.rollout.started_at))}
          <Show when={props.rollout.finished_at}>
            {(at) => <> · {t.finished(formatDateTime(at()))}</>}
          </Show>
        </span>
      </div>
      <Show when={props.rollout.canary || props.rollout.window}>
        <p class="text-fg-muted text-xs">
          <Show when={props.rollout.canary}>
            {(canary) => <>{t.canarySoak(formatDuration(canary().soak_min * 60))} </>}
          </Show>
          <Show when={props.rollout.window}>
            {(window) => t.windowSpan(clockText(window().start_min), clockText(window().end_min))}
          </Show>
        </p>
      </Show>
      <Show when={props.rollout.state === "running" && props.rollout.waiting}>
        {(waiting) => (
          <p role="status" class="flex items-center gap-2 text-fg text-sm">
            <StatusDot tone="info" pulse />
            {waitText(waiting(), props.rollout.nodes[0]?.name ?? "")}
          </p>
        )}
      </Show>
      <Show when={props.rollout.state === "halted"}>
        <div
          role="alert"
          class="flex flex-col gap-1 rounded-control border border-crit/40 bg-crit/10 px-3 py-2.5 text-sm"
        >
          <p class="font-medium text-fg">{t.halted}</p>
          <Show when={props.rollout.error}>{(why) => <p class="text-fg">{why()}</p>}</Show>
          <p class="text-fg-muted text-xs">
            {canaryHalt() ? t.haltedCanary : t.haltedHint(failed()?.name ?? "")}
          </p>
        </div>
      </Show>
      <Show when={props.rollout.state === "cancelled" && props.rollout.error}>
        {(why) => <p class="text-fg-muted text-xs">{why()}</p>}
      </Show>
      <ol class="flex flex-col divide-y divide-edge rounded-control border border-edge">
        <For each={props.rollout.nodes}>
          {(entry, index) => (
            <NodeRow
              entry={entry}
              index={index()}
              canary={props.rollout.canary !== null && index() === 0}
              jobs={props.jobs}
              uploads={props.uploads}
            />
          )}
        </For>
      </ol>
      <RequestError text={error()} />
      <Show when={props.admin && isActive()}>
        <Show
          when={asked()}
          fallback={
            <div class="flex flex-wrap items-center justify-end gap-2">
              <Show when={upgrading()}>
                <p class="mr-auto text-fg-subtle text-xs">{t.cancelBusy}</p>
              </Show>
              <Show when={!upgrading()}>
                <Button size="sm" icon={X} onClick={() => setAsked({ kind: "cancel" })}>
                  {t.cancel}
                </Button>
              </Show>
              <Show when={failed()}>
                {(node) => (
                  <Button
                    size="sm"
                    icon={SkipForward}
                    onClick={() => setAsked({ kind: "skip", node: node().node, name: node().name })}
                  >
                    {t.skip(node().name)}
                  </Button>
                )}
              </Show>
              <Show when={props.rollout.state === "running"}>
                <Button
                  size="sm"
                  icon={Pause}
                  disabled={sending()}
                  onClick={() => void run({ kind: "pause" })}
                >
                  {t.pause}
                </Button>
              </Show>
              <Show when={props.rollout.state !== "running"}>
                <Button
                  size="sm"
                  variant="primary"
                  icon={Play}
                  onClick={() => setAsked({ kind: "resume", name: failed()?.name ?? null })}
                >
                  {t.resume}
                </Button>
              </Show>
            </div>
          }
        >
          {(action) => (
            <div
              role="alert"
              class="flex flex-col gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm"
            >
              <p>{question(action())}</p>
              <div class="flex justify-end gap-2">
                <Button size="sm" disabled={sending()} onClick={() => setAsked(undefined)}>
                  {t.keep}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  disabled={sending()}
                  onClick={() => void run(action())}
                >
                  {action().kind === "cancel"
                    ? t.cancel
                    : action().kind === "skip"
                      ? t.skip(name((action() as { node: number }).node))
                      : t.resume}
                </Button>
              </div>
            </div>
          )}
        </Show>
      </Show>
    </div>
  );
}

/** What a running rollout waits for, for people; `canary` names the first device. */
function waitText(wait: RolloutWait, canary: string): string {
  switch (wait.kind) {
    case "canary":
      return t.wait.canary(canary, formatDateTime(wait.until));
    case "window":
      return t.wait.window(formatDateTime(wait.opens));
    case "busy":
      return t.wait.busy(wait.why);
  }
}

/** One device of the pipeline: its place, image, state, and its job while it matters. */
function NodeRow(props: {
  entry: RolloutNode;
  index: number;
  canary: boolean;
  jobs: JobsState;
  uploads: readonly FirmwareUpload[];
}) {
  const job = () => (props.entry.job === null ? undefined : props.jobs.byId[props.entry.job]);
  const showJob = () =>
    props.entry.state === "upgrading" || props.entry.state === "failed" ? job() : undefined;
  return (
    <li class="flex flex-col gap-2 px-3 py-2.5" aria-label={props.entry.name}>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
        <span class="w-5 text-right text-fg-subtle tabular-nums">{props.index + 1}</span>
        <span class="font-medium">{props.entry.name}</span>
        <Show when={props.canary}>
          <Badge tone="info">{t.canaryTag}</Badge>
        </Show>
        <span class="text-fg-muted text-xs tabular-nums">
          {props.entry.from} → {choiceShort(props.entry.choice, props.uploads)}
        </span>
        <span class="ml-auto">
          <Badge tone={NODE_TONES[props.entry.state]}>
            {t.nodeState[props.entry.state] ?? props.entry.state}
          </Badge>
        </span>
      </div>
      <Show when={props.entry.detail}>
        {(detail) => <p class="pl-8 text-fg-muted text-xs">{detail()}</p>}
      </Show>
      <Show when={showJob()}>
        {(found) => (
          <div class="ml-8 flex flex-col gap-2">
            <div class="rounded-control border border-edge p-2">
              <JobSteps job={found()} />
            </div>
            <Show when={found().error}>{(error) => <JobErrorNote error={error()} />}</Show>
          </div>
        )}
      </Show>
    </li>
  );
}
