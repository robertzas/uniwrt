import * as Popover from "@kobalte/core/popover";
import { BellOff } from "lucide";
import { createSignal, For, Show } from "solid-js";
import { api } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";
import { showToast } from "../../ui/Toast";
import { SNOOZES, snooze } from "./maintenance";
import { clock, errorText } from "./shared";

const t = strings.alerts.maintenance;

/**
 * A quick snooze: a maintenance window from now for an hour, four or a day, for one device (when
 * given) or every device. Alerts that fire within it are recorded silenced.
 */
export function SnoozeMenu(props: {
  node?: { id: number; name: string } | undefined;
  label?: string;
  onSnoozed?: () => void;
  size?: "sm" | "md";
}) {
  const [open, setOpen] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const choose = (ms: number, node: number | null) => {
    setBusy(true);
    api.maintenance
      .create(snooze(ms, node))
      .then((window) => {
        setOpen(false);
        showToast({ title: t.snoozed(clock.format(new Date(window.ends_at))), tone: "info" });
        props.onSnoozed?.();
      })
      .catch((error: unknown) =>
        showToast({ title: t.changeFailed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setBusy(false));
  };
  const scopes = () => [
    ...(props.node ? [{ label: t.snoozeDevice(props.node.name), node: props.node.id }] : []),
    { label: t.snoozeAll, node: null },
  ];

  return (
    <Popover.Root open={open()} onOpenChange={setOpen} placement="bottom-start" gutter={6}>
      <Popover.Trigger
        class={[
          "inline-flex items-center gap-1.5 rounded-control border border-edge bg-surface font-medium text-fg hover:bg-raised data-expanded:bg-raised",
          props.size === "md" ? "h-9 px-3 text-sm" : "h-7 px-2.5 text-xs",
        ]}
      >
        <Icon icon={BellOff} class="size-3.5 text-fg-subtle" />
        {props.label ?? strings.alerts.actions.snooze}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 w-64 rounded-card border border-edge bg-surface p-2 shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
          <Popover.Title class="px-2 pt-1 pb-2 font-medium text-sm">{t.snooze}</Popover.Title>
          <For each={scopes()}>
            {(scope) => (
              <div class="flex flex-col gap-1 px-2 pb-2">
                <Show when={scopes().length > 1}>
                  <p class="text-fg-subtle text-xs">{scope.label}</p>
                </Show>
                <div class="grid grid-cols-3 gap-1">
                  <For each={SNOOZES}>
                    {(ms) => (
                      <button
                        type="button"
                        disabled={busy()}
                        onClick={() => choose(ms, scope.node)}
                        aria-label={`${scope.label}: ${t.snoozeFor(formatDuration(ms / 1000))}`}
                        class="h-8 rounded-control border border-edge font-medium text-xs tabular-nums hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
                      >
                        {formatDuration(ms / 1000)}
                      </button>
                    )}
                  </For>
                </div>
              </div>
            )}
          </For>
          <p class="px-2 pb-1 text-fg-subtle text-xs">{t.snoozeHint}</p>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
