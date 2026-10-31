import { History, Radio } from "lucide";
import { createMemo, createSignal, For, onSettled, Show } from "solid-js";
import { formatDuration, formatTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import type { Replay } from "./replay";

const r = strings.topology.replay;
/** The slider moves in steps of 15 s; its end counts as live. */
const STEP_MS = 15_000;

/** A slider over the last 24 h, its changes marked, which replays the topology at a time. */
export function ReplayBar(props: { replay: Replay }) {
  const [now, setNow] = createSignal(Date.now());
  onSettled(() => {
    const tick = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(tick);
  });
  const start = () => props.replay.timeline()?.start ?? null;
  /** The start, moved back to a whole number of steps before now, so that now is a step. */
  const min = (from: number) => now() - Math.ceil((now() - from) / STEP_MS) * STEP_MS;
  const live = () => props.replay.at() === null;
  /** The changes as tick marks, merged to half a percent of the width. */
  const marks = createMemo(() => {
    const from = start();
    if (from === null) return [];
    const span = Math.max(1, now() - from);
    const steps = new Set<number>();
    for (const at of props.replay.timeline()?.changes ?? []) {
      steps.add(Math.round(((at - from) / span) * 200) / 2);
    }
    return [...steps].filter((step) => step >= 0 && step <= 100);
  });
  const shown = () => {
    const at = props.replay.at();
    return at === null ? r.live : r.at(formatTime(at), formatDuration((now() - at) / 1000));
  };
  return (
    <div class="flex items-center gap-3 rounded-card border border-card-edge bg-surface/95 px-3 py-2 shadow-card glass:backdrop-blur-panel">
      <Icon icon={History} class="size-4 shrink-0 text-fg-subtle" />
      <Show when={start()} fallback={<p class="flex-1 text-fg-subtle text-xs">{r.empty}</p>}>
        {(from) => (
          <div class="relative flex h-6 min-w-0 flex-1 items-center">
            <div aria-hidden="true" class="pointer-events-none absolute inset-x-0 top-0 h-1.5">
              <For each={marks()}>
                {(step) => (
                  <span
                    class="absolute top-0 h-1.5 w-px bg-accent/60"
                    style={{ left: `${step}%` }}
                  />
                )}
              </For>
            </div>
            <input
              type="range"
              min={min(from())}
              max={now()}
              step={STEP_MS}
              value={props.replay.at() ?? now()}
              aria-label={r.label}
              aria-valuetext={shown()}
              class="w-full accent-accent"
              onInput={(event) => {
                const value = Number(event.currentTarget.value);
                props.replay.seek(value >= now() ? null : value);
              }}
            />
          </div>
        )}
      </Show>
      <span
        class={[
          "shrink-0 whitespace-nowrap font-mono text-xs tabular-nums",
          live() ? "text-fg-muted" : "text-fg",
        ]}
        aria-live="polite"
      >
        <Show when={live()}>
          <Icon icon={Radio} class="me-1 inline size-3.5 text-ok" />
        </Show>
        {shown()}
      </span>
      <Show when={!live()}>
        <Button size="sm" onClick={() => props.replay.seek(null)}>
          {r.backToLive}
        </Button>
      </Show>
      <Show when={props.replay.failed()}>
        <p role="alert" class="sr-only">
          {r.failed}
        </p>
      </Show>
    </div>
  );
}
