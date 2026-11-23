import { Radio, Square } from "lucide";
import {
  createEffect,
  createMemo,
  createSignal,
  createUniqueId,
  For,
  Match,
  Show,
  Switch,
} from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { OptionalToolStatus } from "../../../api/gen/OptionalToolStatus";
import { formatBytes, formatCount, formatDuration } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { FormField, INPUT, SELECT } from "../../../ui/FormField";
import {
  ANY,
  type CaptureDraft,
  captureErrors,
  emptyCapture,
  MAX_MIB,
  MAX_SECONDS,
  MAX_SNAPLEN,
  MIN_SNAPLEN,
  toCaptureQuery,
} from "./capture";
import { type CaptureState, captureSession } from "./captureSession";

const t = strings.device.tools.capture;

/** The device's network devices to capture on, each with the interfaces it carries. */
function captureDevices(facts: NodeFacts | null): { name: string; label: string }[] {
  if (!facts) return [];
  const carried = new Map<string, string[]>();
  for (const iface of facts.interfaces) {
    if (iface.device) carried.set(iface.device, [...(carried.get(iface.device) ?? []), iface.name]);
  }
  return facts.devices
    .map((device) => device.name)
    .filter((name) => name !== "lo")
    .sort((a, b) => a.localeCompare(b))
    .map((name) => {
      const names = carried.get(name);
      return { name, label: names ? `${name} (${names.join(", ")})` : name };
    });
}

/**
 * A packet capture's form and progress (D-142): tcpdump on the device into a `.pcap`, read by the
 * browser as it arrives (so that tcpdump's refusal shows here, and *Stop* saves what came) and
 * saved when it ends.
 */
export function CaptureCard(props: {
  node: Node;
  facts: NodeFacts | null;
  admin: boolean;
  online: boolean;
  tools: readonly OptionalToolStatus[] | undefined;
}) {
  const id = createUniqueId();
  const session = createMemo(() => captureSession(props.node.id));
  const state = () => session().state();
  const [draft, setDraft] = createSignal<CaptureDraft>(emptyCapture());
  const [tried, setTried] = createSignal(false);
  const errors = createMemo(() => captureErrors(draft()));
  const error = (field: keyof CaptureDraft) =>
    tried() || field === "filter" ? errors()[field] : undefined;
  const devices = createMemo(() => captureDevices(props.facts));
  const update = (patch: Partial<CaptureDraft>) => {
    setDraft((now) => ({ ...now, ...patch }));
    session().clear();
  };
  const missing = () =>
    props.tools?.some((tool) => tool.tool === "tcpdump" && !tool.installed) ?? false;
  const blocked = () =>
    !props.admin ? t.adminOnly : !props.online ? t.offline : missing() ? t.missing : undefined;
  const running = () => state()?.phase === "starting" || state()?.phase === "capturing";

  // A second's tick while it captures, for the time it has run.
  const [now, setNow] = createSignal(Date.now());
  createEffect(
    () => state()?.phase === "capturing",
    (capturing) => {
      if (!capturing) return;
      setNow(Date.now());
      const timer = setInterval(() => setNow(Date.now()), 1000);
      return () => clearInterval(timer);
    },
  );

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    setTried(true);
    const query = toCaptureQuery(draft());
    if (!query || running() || blocked()) return;
    void session().start(query);
  };

  const number = (
    field: "seconds" | "maxMib" | "snaplen",
    label: string,
    hint: string,
    min: number,
    max: number,
  ) => (
    <FormField label={label} for={`${id}-${field}`} hint={hint} error={error(field)}>
      <input
        id={`${id}-${field}`}
        type="number"
        min={min}
        max={max}
        class={[INPUT, error(field) ? "border-crit" : "border-edge"]}
        value={draft()[field]}
        aria-invalid={error(field) ? "true" : undefined}
        onInput={(event) => update({ [field]: event.currentTarget.value })}
      />
    </FormField>
  );

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <form class="flex flex-col gap-4 px-4 pb-4" onSubmit={submit} novalidate>
        <fieldset disabled={!props.admin || running()} class="flex min-w-0 flex-col gap-4">
          <FormField label={t.interface} for={`${id}-interface`} error={error("interface")}>
            <select
              id={`${id}-interface`}
              class={SELECT}
              value={draft().interface}
              onChange={(event) => update({ interface: event.currentTarget.value })}
            >
              <option value={ANY}>{t.any}</option>
              <For each={devices()}>
                {(device) => <option value={device.name}>{device.label}</option>}
              </For>
            </select>
          </FormField>
          <FormField
            label={t.filter}
            for={`${id}-filter`}
            hint={t.filterHint}
            error={error("filter")}
          >
            <input
              id={`${id}-filter`}
              class={[INPUT, "font-mono", error("filter") ? "border-crit" : "border-edge"]}
              value={draft().filter}
              autocapitalize="none"
              spellcheck={false}
              aria-invalid={error("filter") ? "true" : undefined}
              onInput={(event) => update({ filter: event.currentTarget.value })}
            />
          </FormField>
          <div class="grid gap-4 sm:grid-cols-3">
            {number("seconds", t.seconds, t.secondsHint, 1, MAX_SECONDS)}
            {number("maxMib", t.maxMib, t.maxMibHint, 1, MAX_MIB)}
            {number("snaplen", t.snaplen, t.snaplenHint, MIN_SNAPLEN, MAX_SNAPLEN)}
          </div>
        </fieldset>
        <Progress state={state()} now={now()} />
        <div class="flex flex-wrap items-center justify-end gap-3">
          <Show when={!running() && blocked()}>
            <p class="mr-auto text-fg-subtle text-xs">{blocked()}</p>
          </Show>
          <Show
            when={running()}
            fallback={
              <Button
                type="submit"
                variant="primary"
                icon={Radio}
                disabled={blocked() !== undefined}
              >
                {t.start}
              </Button>
            }
          >
            <Button
              type="button"
              variant="secondary"
              icon={Square}
              onClick={() => session().stop()}
            >
              {state()?.phase === "starting" ? t.starting : t.stop}
            </Button>
          </Show>
        </div>
      </form>
    </Card>
  );
}

type Phase<P extends NonNullable<CaptureState>["phase"]> = Extract<CaptureState, { phase: P }>;

/**
 * Where the capture is: running (time and size against their caps), saved, or why not. The views
 * read `props.state` themselves: a `Match` callback's value stays the first one while it holds.
 */
function Progress(props: { state: CaptureState | undefined; now: number }) {
  return (
    <Switch>
      <Match when={props.state?.phase === "capturing"}>
        <Capturing run={props.state as Phase<"capturing">} now={props.now} />
      </Match>
      <Match when={props.state?.phase === "saved"}>
        <Saved saved={props.state as Phase<"saved">} />
      </Match>
      <Match when={props.state?.phase === "empty"}>
        <p role="status" class="text-fg-subtle text-sm">
          {t.empty}
        </p>
      </Match>
      <Match when={props.state?.phase === "failed"}>
        <div role="alert" class="text-sm">
          <p class="font-medium text-crit">{t.failed}</p>
          <p class="text-fg-muted">{(props.state as Phase<"failed">).error}</p>
        </div>
      </Match>
    </Switch>
  );
}

function Capturing(props: { run: Phase<"capturing">; now: number }) {
  const elapsed = () => Math.max(0, Math.round((props.now - props.run.started) / 1000));
  const share = () =>
    Math.min(1, Math.max(elapsed() / props.run.seconds, props.run.bytes / props.run.maxBytes));
  return (
    <div role="status" class="flex flex-col gap-2 text-sm">
      <div class="flex items-center gap-2">
        <StatusDot tone="info" pulse />
        <span class="font-medium">{t.running}</span>
        <span class="ml-auto text-fg-subtle text-xs tabular-nums">
          {t.elapsed(formatDuration(elapsed()), formatDuration(props.run.seconds))}
        </span>
      </div>
      <div
        role="progressbar"
        aria-label={t.running}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(share() * 100)}
        class="h-1.5 overflow-hidden rounded-full bg-raised"
      >
        <div class="h-full bg-accent transition-[width]" style={{ width: `${share() * 100}%` }} />
      </div>
      <p class="text-fg-subtle text-xs tabular-nums">
        {t.progress(
          props.run.packets,
          formatCount(props.run.packets),
          formatBytes(props.run.bytes),
        )}
        {" · "}
        {t.keepsRunning}
      </p>
    </div>
  );
}

function Saved(props: { saved: Phase<"saved"> }) {
  return (
    <div role="status" class="flex flex-col gap-1 text-sm">
      <p class="flex items-center gap-2">
        <StatusDot tone={props.saved.error ? "warn" : "ok"} />
        <span class="min-w-0 truncate font-medium">{t.saved(props.saved.name)}</span>
      </p>
      <p class="text-fg-subtle text-xs tabular-nums">
        {t.progress(
          props.saved.packets,
          formatCount(props.saved.packets),
          formatBytes(props.saved.bytes),
        )}
      </p>
      <Show when={props.saved.error}>
        <p class="text-warn text-xs">{t.broke(props.saved.error ?? "")}</p>
      </Show>
    </div>
  );
}
