import { createSignal, For, Show } from "solid-js";
import type { RfSettings } from "../../api/gen/RfSettings";
import type { RfView } from "../../api/gen/RfView";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Segmented } from "../../ui/Segmented";
import { showToast } from "../../ui/Toast";

const t = strings.rf.scanning;
const INTERVALS = [1, 2, 4, 8, 12, 24] as const;

/** Starts scans of every radio, or of one; a toast says how many were queued or why not. */
export function startScans(node?: number, radio?: string): Promise<void> {
  return api.rf
    .scan({ node, radio })
    .then((answer) => {
      showToast({ title: t.queued(answer.queued.length), tone: "ok" });
    })
    .catch((error: unknown) => {
      showToast({ title: t.failed, description: errorText(error), tone: "crit" });
    });
}

/**
 * The page's scanning controls: how many radios have been scanned, and for admins the
 * background scans on or off, their interval and a scan of every radio now.
 */
export function Scanning(props: { view: RfView; admin: boolean }) {
  const [saving, setSaving] = createSignal(false);
  const [scanning, setScanning] = createSignal(false);
  const scanned = () => props.view.radios.filter((r) => r.last !== null).length;
  // The usual intervals, and the one set when it is another (the API takes 1 to 24 h).
  const intervals = () =>
    [...new Set<number>([...INTERVALS, props.view.settings.interval_h])].sort((a, b) => a - b);
  const save = (settings: RfSettings) => {
    setSaving(true);
    api.rf
      .settings(settings)
      .then(() => showToast({ title: t.saved, tone: "ok" }))
      .catch((error: unknown) =>
        showToast({ title: t.saveFailed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setSaving(false));
  };
  const scanAll = () => {
    setScanning(true);
    void startScans().finally(() => setScanning(false));
  };
  return (
    <div class="flex flex-wrap items-center justify-end gap-x-4 gap-y-2 text-sm">
      <span class="text-fg-muted text-xs tabular-nums">
        {t.summary(scanned(), props.view.radios.length)}
      </span>
      <Show
        when={props.admin}
        fallback={
          <span class="text-fg-muted text-xs">
            {`${t.background}: ${props.view.settings.scanning ? t.every(props.view.settings.interval_h) : t.off}`}
          </span>
        }
      >
        <div class="flex items-center gap-2" title={t.hint}>
          <span class="text-fg-muted text-xs">{t.background}</span>
          <Segmented
            label={t.background}
            value={props.view.settings.scanning ? "on" : "off"}
            onChange={(value) => save({ ...props.view.settings, scanning: value === "on" })}
            options={[
              { value: "on", label: t.on },
              { value: "off", label: t.off },
            ]}
          />
          <Show when={props.view.settings.scanning}>
            <label class="flex items-center gap-1.5">
              <span class="sr-only">{t.interval}</span>
              <select
                class="h-7 rounded-control border border-edge bg-surface px-1.5 text-fg text-xs"
                value={String(props.view.settings.interval_h)}
                disabled={saving()}
                onChange={(event) =>
                  save({ ...props.view.settings, interval_h: Number(event.currentTarget.value) })
                }
              >
                <For each={intervals()}>
                  {(hours) => <option value={String(hours)}>{t.every(hours)}</option>}
                </For>
              </select>
            </label>
          </Show>
        </div>
        <Button
          size="sm"
          variant="secondary"
          disabled={scanning() || props.view.radios.length === 0}
          title={t.nowHint}
          onClick={scanAll}
        >
          {t.now}
        </Button>
      </Show>
    </div>
  );
}
