import { LoaderCircle, TriangleAlert } from "lucide";
import { createEffect, For, Show } from "solid-js";
import type { UpgradeChoice } from "../../api/gen/UpgradeChoice";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { api, errorText } from "../../lib/api";
import { type FirmwareRow, tightTmp } from "../../lib/firmware";
import { choiceLabel, choiceValue, rowChoices } from "../../lib/rollouts";
import { strings } from "../../lib/strings";
import { Checkbox } from "../../ui/Checkbox";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { PlanView } from "./PlanView";

const r = strings.firmware.rollouts.dialog;
const d = strings.firmware.dialog;

/** A plan as it is read: under way, read, or refused. */
export interface PlanState {
  plan?: UpgradePlan;
  error?: string;
}

/** The key of a node's plan with a choice: a changed choice is a different plan. */
export function planKey(node: number, choice: UpgradeChoice): string {
  return `${node}|${choiceValue(choice)}`;
}

/**
 * A device in the rollout dialog (D-125): chosen or not (a device in observe mode says why it
 * can't be), its image (any of its offers, as the upgrade dialog offers them), and, once chosen,
 * its plan read from the device and summed up, the whole plan a click away.
 */
export function RolloutDevice(props: {
  row: FirmwareRow;
  selected: boolean;
  onSelect: (selected: boolean) => void;
  choice: UpgradeChoice;
  onChoose: (choice: UpgradeChoice) => void;
  plan: PlanState | undefined;
  onPlan: (key: string, state: PlanState) => void;
  disabled: boolean;
}) {
  const name = () => props.row.node?.name ?? `#${props.row.firmware.node}`;
  const managed = () => props.row.node?.mode === "managed";
  const choices = () => rowChoices(props.row);

  // The chosen image's plan, read again whenever the choice changes while the device is chosen.
  createEffect(
    () =>
      props.selected && managed() ? planKey(props.row.firmware.node, props.choice) : undefined,
    (key) => {
      if (!key) return;
      const node = props.row.firmware.node;
      const controller = new AbortController();
      props.onPlan(key, {});
      api.firmware
        .plan(node, props.choice, controller.signal)
        .then((plan) => {
          props.onPlan(key, { plan });
        })
        .catch((err) => {
          if (!controller.signal.aborted) props.onPlan(key, { error: errorText(err) });
        });
      return () => controller.abort();
    },
  );

  const summary = (plan: UpgradePlan) => {
    if (plan.upload || plan.source === "github") return r.flashed;
    const kept =
      plan.source === "official" ? d.installed(plan.kept.length) : d.kept(plan.kept.length);
    return plan.dropped.length > 0 ? `${kept} · ${r.dropped(plan.dropped.length)}` : kept;
  };

  return (
    <li class="flex flex-col gap-2 px-3 py-2.5">
      <div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
        <Checkbox
          label={name()}
          hint={
            managed()
              ? `${props.row.firmware.version || "?"} · ${props.row.node?.model ?? props.row.firmware.board}`
              : r.observe
          }
          checked={props.selected && managed()}
          disabled={!managed() || props.disabled}
          onChange={props.onSelect}
        />
        <Show when={props.selected && managed()}>
          <select
            aria-label={r.image(name())}
            class="h-8 max-w-full rounded-control border border-edge bg-surface px-2 text-fg text-sm"
            value={choiceValue(props.choice)}
            disabled={props.disabled}
            onChange={(event) => {
              const value = event.currentTarget.value;
              const found = choices().find((choice) => choiceValue(choice) === value);
              if (found) props.onChoose(found);
            }}
          >
            <For each={choices()}>
              {(choice) => (
                <option value={choiceValue(choice)}>{choiceLabel(choice, props.row)}</option>
              )}
            </For>
          </select>
        </Show>
      </div>
      <Show when={props.selected && managed()}>
        <div class="pl-7">
          <Show
            when={props.plan?.plan}
            fallback={
              <Show
                when={props.plan?.error}
                fallback={
                  <p role="status" class="flex items-center gap-2 text-fg-muted text-xs">
                    <Icon icon={LoaderCircle} class="size-3.5 animate-spin text-accent" />
                    {r.planning}
                  </p>
                }
              >
                {(error) => <RequestError text={error()} />}
              </Show>
            }
          >
            {(plan) => (
              <details class="group text-xs">
                <summary
                  class={[
                    "flex cursor-pointer items-center gap-1.5",
                    plan().dropped.length > 0 || tightTmp(plan()) ? "text-warn" : "text-fg-muted",
                  ]}
                >
                  <Show when={plan().dropped.length > 0 || tightTmp(plan())}>
                    <Icon icon={TriangleAlert} class="size-3.5 shrink-0" />
                  </Show>
                  {summary(plan())}
                  <span class="text-accent group-open:hidden">· {r.showPlan}</span>
                </summary>
                <div class="mt-2 rounded-control border border-edge p-3">
                  <PlanView plan={plan()} />
                </div>
              </details>
            )}
          </Show>
        </div>
      </Show>
    </li>
  );
}
