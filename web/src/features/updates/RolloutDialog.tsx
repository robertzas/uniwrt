import { Rocket } from "lucide";
import { createMemo, createSignal, For, Show, untrack } from "solid-js";
import type { Rollout } from "../../api/gen/Rollout";
import type { RolloutBody } from "../../api/gen/RolloutBody";
import type { UpgradeChoice } from "../../api/gen/UpgradeChoice";
import { api, errorText } from "../../lib/api";
import type { FirmwareRow } from "../../lib/firmware";
import { clockText, nightlyTimes } from "../../lib/format";
import {
  DEFAULT_SOAK,
  DEFAULT_WINDOW,
  defaultChoice,
  previewOrder,
  SOAKS,
} from "../../lib/rollouts";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { type PlanState, planKey, RolloutDevice } from "./RolloutDevice";

const t = strings.firmware.rollouts;
const r = t.dialog;

const SELECT =
  "h-8 rounded-control border border-edge bg-surface px-2 text-fg text-sm tabular-nums disabled:opacity-60";

/**
 * Starts a rolling upgrade (D-125): the managed devices with an upgrade on offer, those that can
 * upgrade preselected, each with its newest offer from its own source and changeable; every
 * chosen device's plan read and shown; the expected order; the canary (on for 2+ devices, 1 h) and
 * the maintenance window (off, 03:30–05:30 when on). Start asks first, then hands the rollout, as
 * the controller ordered it, to `onStarted`.
 */
export function RolloutDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The devices an upgrade can be prepared for. */
  rows: readonly FirmwareRow[];
  onStarted: (rollout: Rollout) => void;
}) {
  const managed = (row: FirmwareRow) => row.node?.mode === "managed";
  const initial = untrack(() => props.rows);
  const [selected, setSelected] = createSignal<ReadonlySet<number>>(
    new Set(
      initial
        .filter((row) => managed(row) && row.state === "available")
        .map((row) => row.firmware.node),
    ),
  );
  const [choices, setChoices] = createSignal<ReadonlyMap<number, UpgradeChoice>>(
    new Map(
      initial.flatMap((row) => {
        const choice = defaultChoice(row);
        return choice ? [[row.firmware.node, choice] as const] : [];
      }),
    ),
  );
  const [plans, setPlans] = createSignal<Readonly<Record<string, PlanState>>>({});
  const [canaryOn, setCanaryOn] = createSignal<boolean>();
  const [canaryNode, setCanaryNode] = createSignal<number | null>(null);
  const [soak, setSoak] = createSignal<number>(DEFAULT_SOAK);
  const [windowOn, setWindowOn] = createSignal(false);
  const [start, setStart] = createSignal<number>(DEFAULT_WINDOW.start_min);
  const [end, setEnd] = createSignal<number>(DEFAULT_WINDOW.end_min);
  const [asking, setAsking] = createSignal(false);
  const [sending, setSending] = createSignal(false);
  const [error, setError] = createSignal<string>();

  /** The chosen devices with their images, as the dialog lists them. */
  const chosen = createMemo(() =>
    props.rows.flatMap((row) => {
      const choice = choices().get(row.firmware.node);
      return selected().has(row.firmware.node) && managed(row) && choice ? [{ row, choice }] : [];
    }),
  );
  const nameOf = (row: FirmwareRow) => row.node?.name ?? `#${row.firmware.node}`;
  const entries = () =>
    chosen().map(({ row }) => ({
      node: row.firmware.node,
      name: nameOf(row),
      gateway: row.node?.role === "gateway",
    }));
  const several = () => chosen().length > 1;
  const canary = () => several() && (canaryOn() ?? true);
  /** The canary picked, while it is still a chosen device other than the gateway. */
  const pickedCanary = () => {
    const node = canaryNode();
    return entries().some((entry) => entry.node === node && !entry.gateway) ? node : null;
  };
  const order = () => previewOrder(entries(), canary() ? pickedCanary() : undefined);
  const windowValid = () => !windowOn() || start() !== end();
  const planOf = (node: number, choice: UpgradeChoice) => plans()[planKey(node, choice)];
  const planned = () =>
    chosen().every(({ row, choice }) => planOf(row.firmware.node, choice)?.plan !== undefined);
  const ready = () => chosen().length > 0 && planned() && windowValid();

  const toggle = (node: number, on: boolean) => {
    const next = new Set(selected());
    if (on) next.add(node);
    else next.delete(node);
    setSelected(next);
    setAsking(false);
  };
  const choose = (node: number, choice: UpgradeChoice) => {
    setChoices(new Map(choices()).set(node, choice));
    setAsking(false);
  };
  const body = (): RolloutBody => ({
    nodes: chosen().map(({ row, choice }) => ({ node: row.firmware.node, choice })),
    canary: canary() ? { node: pickedCanary(), soak_min: soak() } : null,
    window: windowOn() ? { start_min: start(), end_min: end() } : null,
  });
  const send = async () => {
    setSending(true);
    setError(undefined);
    try {
      const rollout = await api.firmware.rollouts.create(body());
      props.onStarted(rollout);
      props.onOpenChange(false);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSending(false);
    }
  };
  const duration = (minutes: number) => (minutes < 60 ? r.minutes(minutes) : r.hours(minutes / 60));

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={r.title}
      description={r.description}
      size="lg"
    >
      <div class="flex flex-col gap-5 px-5 pb-5">
        <section class="flex flex-col gap-2" aria-label={r.devices}>
          <h3 class="font-medium text-fg text-sm">{r.devices}</h3>
          <Show
            when={props.rows.length > 0}
            fallback={<p class="text-fg-subtle text-sm">{r.none}</p>}
          >
            <ul class="flex flex-col divide-y divide-edge rounded-control border border-edge">
              <For each={props.rows}>
                {(row) => (
                  <Show when={choices().get(row.firmware.node)}>
                    {(choice) => (
                      <RolloutDevice
                        row={row}
                        selected={selected().has(row.firmware.node)}
                        onSelect={(on) => toggle(row.firmware.node, on)}
                        choice={choice()}
                        onChoose={(next) => choose(row.firmware.node, next)}
                        plan={planOf(row.firmware.node, choice())}
                        onPlan={(key, state) => setPlans((known) => ({ ...known, [key]: state }))}
                        disabled={sending()}
                      />
                    )}
                  </Show>
                )}
              </For>
            </ul>
          </Show>
        </section>

        <Show when={chosen().length > 0}>
          <section class="flex flex-col gap-2" aria-label={r.order}>
            <h3 class="font-medium text-fg text-sm">{r.order}</h3>
            <ol class="flex flex-col gap-1 text-sm">
              <For each={order()}>
                {(entry, index) => (
                  <li class="flex items-center gap-2">
                    <span class="w-5 text-right text-fg-subtle tabular-nums">{index() + 1}</span>
                    <span>{entry.name}</span>
                    <Show when={canary() && index() === 0}>
                      <Badge tone="info">{t.canaryTag}</Badge>
                    </Show>
                  </li>
                )}
              </For>
            </ol>
            <Show when={chosen().length > 2 || (canary() && pickedCanary() === null)}>
              <p class="text-fg-subtle text-xs">{r.orderHint}</p>
            </Show>
          </section>
        </Show>

        <Show when={several()}>
          <section class="flex flex-col gap-2" aria-label={r.canary}>
            <Checkbox
              label={r.canary}
              hint={r.canaryHint}
              checked={canary()}
              disabled={sending()}
              onChange={setCanaryOn}
            />
            <Show when={canary()}>
              <div class="flex flex-wrap gap-4 pl-7">
                <label class="flex items-center gap-2 text-xs">
                  <span class="text-fg-muted">{r.canaryNode}</span>
                  <select
                    class={SELECT}
                    value={String(pickedCanary() ?? "")}
                    disabled={sending()}
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setCanaryNode(value === "" ? null : Number(value));
                    }}
                  >
                    <option value="">{r.canaryAuto}</option>
                    <For each={entries().filter((entry) => !entry.gateway)}>
                      {(entry) => <option value={String(entry.node)}>{entry.name}</option>}
                    </For>
                  </select>
                </label>
                <label class="flex items-center gap-2 text-xs">
                  <span class="text-fg-muted">{r.soak}</span>
                  <select
                    class={SELECT}
                    value={String(soak())}
                    disabled={sending()}
                    onChange={(event) => setSoak(Number(event.currentTarget.value))}
                  >
                    <For each={SOAKS}>
                      {(minutes) => <option value={String(minutes)}>{duration(minutes)}</option>}
                    </For>
                  </select>
                </label>
              </div>
            </Show>
          </section>
        </Show>

        <Show when={chosen().length > 0}>
          <section class="flex flex-col gap-2" aria-label={r.window}>
            <Checkbox
              label={r.window}
              hint={r.windowHint}
              checked={windowOn()}
              disabled={sending()}
              onChange={setWindowOn}
            />
            <Show when={windowOn()}>
              <div class="flex flex-wrap gap-4 pl-7">
                <TimeSelect
                  label={r.from}
                  value={start()}
                  onChange={setStart}
                  disabled={sending()}
                />
                <TimeSelect label={r.to} value={end()} onChange={setEnd} disabled={sending()} />
              </div>
              <Show when={!windowValid()}>
                <p class="pl-7 text-crit text-xs">{r.windowSame}</p>
              </Show>
            </Show>
          </section>
        </Show>

        <Show when={asking()}>
          <div
            role="alert"
            class="flex flex-col gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm"
          >
            <p>{r.confirm(chosen().length)}</p>
          </div>
        </Show>
        <RequestError text={error()} />
        <div class="flex justify-end gap-2">
          <Button
            size="sm"
            onClick={() => (asking() ? setAsking(false) : props.onOpenChange(false))}
          >
            {r.cancel}
          </Button>
          <Show
            when={asking()}
            fallback={
              <Button
                size="sm"
                variant="primary"
                icon={Rocket}
                disabled={!ready()}
                onClick={() => setAsking(true)}
              >
                {r.startButton(chosen().length)}
              </Button>
            }
          >
            <Button
              size="sm"
              variant="primary"
              icon={Rocket}
              disabled={sending() || !ready()}
              onClick={() => void send()}
            >
              {sending() ? r.starting : r.startNow}
            </Button>
          </Show>
        </div>
      </div>
    </Dialog>
  );
}

/** A time of day on the half hour. */
function TimeSelect(props: {
  label: string;
  value: number;
  onChange: (minutes: number) => void;
  disabled: boolean;
}) {
  return (
    <label class="flex items-center gap-2 text-xs">
      <span class="text-fg-muted">{props.label}</span>
      <select
        class={SELECT}
        value={String(props.value)}
        disabled={props.disabled}
        onChange={(event) => props.onChange(Number(event.currentTarget.value))}
      >
        <For each={nightlyTimes(props.value)}>
          {(minutes) => <option value={String(minutes)}>{clockText(minutes)}</option>}
        </For>
      </select>
    </label>
  );
}
