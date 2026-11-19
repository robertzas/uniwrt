import { Plus, X } from "lucide";
import { For, Show } from "solid-js";
import { strings } from "../lib/strings";
import { Button } from "./Button";
import { Checkbox } from "./Checkbox";
import { INPUT } from "./FormField";
import { type Day, WEEK, Weekdays } from "./Weekdays";

const t = strings.schedule;

/** A span as typed: `from` and `to` are `HH:MM` (`lib/schedule`'s `SpanDraft`). */
export interface ScheduleSpan {
  days: Day[];
  from: string;
  to: string;
}

const CLOCK = /^([01]\d|2[0-3]):[0-5]\d$/;

/**
 * A weekly schedule (lib/schedule.ts): whether there is one, and the times of the week it is on,
 * in the site's time zone. A time that ends at or before its start runs into the next day. WLANs
 * (D-083) and the LEDs-off schedule (D-138) use it; `fresh` is the span "Add a time" adds.
 */
export function ScheduleField(props: {
  title: string;
  label: string;
  hint: string;
  scheduled: boolean;
  spans: readonly ScheduleSpan[];
  error: string | undefined;
  onChange: (next: { scheduled?: boolean; spans?: ScheduleSpan[] }) => void;
  fresh?: () => ScheduleSpan;
  disabled?: boolean;
}) {
  const fresh = () => props.fresh?.() ?? { days: [...WEEK], from: "07:00", to: "22:00" };
  const set = (index: number, next: Partial<ScheduleSpan>) =>
    props.onChange({
      spans: props.spans.map((span, i) => (i === index ? { ...span, ...next } : span)),
    });
  /** What the two times mean when the end isn't after the start. */
  const wraps = (span: ScheduleSpan) => {
    // Valid `HH:MM` times compare as their strings do.
    if (!CLOCK.test(span.from) || !CLOCK.test(span.to) || span.from < span.to) return undefined;
    return span.from === span.to ? t.wholeDay : t.nextDay;
  };
  return (
    <section class="flex flex-col gap-3" aria-label={props.title}>
      <h3 class="font-semibold text-sm">{props.title}</h3>
      <Checkbox
        checked={props.scheduled}
        onChange={(scheduled) => props.onChange({ scheduled })}
        label={props.label}
        hint={props.hint}
        disabled={props.disabled}
      />
      <Show when={props.scheduled}>
        <ul class="flex flex-col gap-2">
          <For each={props.spans}>
            {(span, index) => (
              <li class="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-control border border-edge p-2.5">
                <Weekdays
                  days={span.days}
                  onChange={(days) => set(index(), { days })}
                  disabled={props.disabled}
                />
                <div class="flex items-center gap-2 text-fg-muted text-xs">
                  <input
                    type="time"
                    aria-label={t.from}
                    value={span.from}
                    disabled={props.disabled}
                    onInput={(event) => set(index(), { from: event.currentTarget.value })}
                    class={[INPUT, "h-8 w-28 border-edge"]}
                  />
                  –
                  <input
                    type="time"
                    aria-label={t.to}
                    value={span.to}
                    disabled={props.disabled}
                    onInput={(event) => set(index(), { to: event.currentTarget.value })}
                    class={[INPUT, "h-8 w-28 border-edge"]}
                  />
                  <Show when={wraps(span)}>
                    {(text) => <span class="whitespace-nowrap">{text()}</span>}
                  </Show>
                </div>
                <span class="flex-1" />
                <Show when={!props.disabled}>
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={X}
                    aria-label={t.removeSpan}
                    onClick={() => props.onChange({ spans: props.spans.filter((s) => s !== span) })}
                  />
                </Show>
              </li>
            )}
          </For>
        </ul>
        <div class="flex items-center gap-3">
          <Show when={!props.disabled}>
            <Button
              size="sm"
              icon={Plus}
              onClick={() => props.onChange({ spans: [...props.spans, fresh()] })}
            >
              {t.addSpan}
            </Button>
          </Show>
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
