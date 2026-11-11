import { Plus, X } from "lucide";
import { For, Show } from "solid-js";
import type { Weekday } from "../../api/gen/Weekday";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { INPUT } from "../../ui/FormField";
import { minutesOf, newSpan, type SpanDraft, WEEKDAYS } from "./draft";

const t = strings.wifi.editor;
const days = strings.wifi.schedule;

/** A day's toggle and the bands' in the editor: pressed is on. */
export const TOGGLE =
  "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-control border px-2.5 text-xs font-medium transition-colors focus-visible:outline-2 focus-visible:outline-accent";
export const toggled = (on: boolean) =>
  on ? "border-accent bg-accent/15 text-fg" : "border-edge text-fg-muted hover:bg-raised";

/**
 * A WLAN's schedule (wifi.md, WLANs; D-083): whether it has one, and the times of the week it
 * is on, in the site's time zone. A time that ends at or before its start runs into the next day.
 */
export function ScheduleField(props: {
  scheduled: boolean;
  spans: readonly SpanDraft[];
  zone: string;
  error: string | undefined;
  onChange: (next: { scheduled?: boolean; spans?: SpanDraft[] }) => void;
}) {
  const set = (index: number, next: Partial<SpanDraft>) =>
    props.onChange({
      spans: props.spans.map((span, i) => (i === index ? { ...span, ...next } : span)),
    });
  const toggle = (index: number, span: SpanDraft, day: Weekday) =>
    set(index, {
      days: span.days.includes(day) ? span.days.filter((d) => d !== day) : [...span.days, day],
    });
  /** What the two times mean when the end isn't after the start. */
  const wraps = (span: SpanDraft) => {
    const from = minutesOf(span.from);
    const to = minutesOf(span.to);
    if (from === undefined || to === undefined || from < to) return undefined;
    return from === to ? t.wholeDay : t.nextDay;
  };
  return (
    <section class="flex flex-col gap-3" aria-label={t.schedule}>
      <h3 class="font-semibold text-sm">{t.schedule}</h3>
      <Checkbox
        checked={props.scheduled}
        onChange={(scheduled) => props.onChange({ scheduled })}
        label={t.scheduled}
        hint={t.scheduledHint(props.zone)}
      />
      <Show when={props.scheduled}>
        <ul class="flex flex-col gap-2">
          <For each={props.spans}>
            {(span, index) => (
              <li class="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-control border border-edge p-2.5">
                <div class="flex flex-wrap gap-1">
                  <For each={WEEKDAYS}>
                    {(day) => (
                      <button
                        type="button"
                        aria-pressed={span.days.includes(day) ? "true" : "false"}
                        aria-label={days.dayNames[day]}
                        class={[TOGGLE, "px-2", toggled(span.days.includes(day))]}
                        onClick={() => toggle(index(), span, day)}
                      >
                        {days.days[day]}
                      </button>
                    )}
                  </For>
                </div>
                <div class="flex items-center gap-2 text-fg-muted text-xs">
                  <input
                    type="time"
                    aria-label={t.from}
                    value={span.from}
                    onInput={(event) => set(index(), { from: event.currentTarget.value })}
                    class={[INPUT, "h-8 w-28 border-edge"]}
                  />
                  –
                  <input
                    type="time"
                    aria-label={t.to}
                    value={span.to}
                    onInput={(event) => set(index(), { to: event.currentTarget.value })}
                    class={[INPUT, "h-8 w-28 border-edge"]}
                  />
                  <Show when={wraps(span)}>{(text) => <span>{text()}</span>}</Show>
                </div>
                <span class="flex-1" />
                <Button
                  size="sm"
                  variant="ghost"
                  icon={X}
                  aria-label={t.removeSpan}
                  onClick={() => props.onChange({ spans: props.spans.filter((s) => s !== span) })}
                />
              </li>
            )}
          </For>
        </ul>
        <div class="flex items-center gap-3">
          <Button
            size="sm"
            icon={Plus}
            onClick={() => props.onChange({ spans: [...props.spans, newSpan()] })}
          >
            {t.addSpan}
          </Button>
          <Show when={props.error}>
            <p class="text-crit text-xs" role="alert">
              {props.error}
            </p>
          </Show>
        </div>
      </Show>
    </section>
  );
}
