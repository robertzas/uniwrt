import { createSignal, For, Show } from "solid-js";
import type { RfSettings } from "../../api/gen/RfSettings";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Checkbox } from "../../ui/Checkbox";
import { showToast } from "../../ui/Toast";

const t = strings.rf.plan.nightly;

/** Minutes after midnight as a 24-hour clock, such as `03:30`. */
export function clockText(minutes: number): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
}

/** The start times offered: every half hour, and the one set when it is another. */
export function nightlyTimes(current: number): number[] {
  const halves = Array.from({ length: 48 }, (_, i) => i * 30);
  return [...new Set([...halves, current])].sort((a, b) => a - b);
}

/**
 * The nightly optimizer (features/wifi.md, RF planner: Apply; D-115): on or off, and when its
 * two-hour window opens, local time. Viewers read them.
 */
export function Nightly(props: { settings: RfSettings; admin: boolean }) {
  const [saving, setSaving] = createSignal(false);
  const save = (settings: RfSettings) => {
    setSaving(true);
    api.rf
      .settings(settings)
      .then(() => showToast({ title: t.saved, tone: "ok" }))
      .catch((error: unknown) =>
        showToast({ title: t.failed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setSaving(false));
  };
  return (
    <Show
      when={props.admin}
      fallback={
        <span class="text-fg-muted text-xs">
          {props.settings.nightly ? t.at(clockText(props.settings.nightly_at_min)) : t.off}
        </span>
      }
    >
      <div class="flex items-center gap-2" title={t.hint}>
        <Checkbox
          label={t.label}
          checked={props.settings.nightly}
          disabled={saving()}
          onChange={(nightly) => save({ ...props.settings, nightly })}
        />
        <Show when={props.settings.nightly}>
          <label class="flex items-center gap-1.5">
            <span class="sr-only">{t.time}</span>
            <select
              class="h-7 rounded-control border border-edge bg-surface px-1.5 text-fg text-xs tabular-nums"
              value={String(props.settings.nightly_at_min)}
              disabled={saving()}
              onChange={(event) =>
                save({ ...props.settings, nightly_at_min: Number(event.currentTarget.value) })
              }
            >
              <For each={nightlyTimes(props.settings.nightly_at_min)}>
                {(minutes) => <option value={String(minutes)}>{clockText(minutes)}</option>}
              </For>
            </select>
          </label>
        </Show>
      </div>
    </Show>
  );
}
