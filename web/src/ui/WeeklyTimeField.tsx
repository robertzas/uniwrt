import { Show } from "solid-js";
import { strings } from "../lib/strings";
import { INPUT } from "./FormField";
import { type Day, Weekdays } from "./Weekdays";

const t = strings.schedule;

/** Days and a time as typed: `at` is `HH:MM` (`lib/schedule`'s `TimeDraft`). */
export interface WeeklyTime {
  days: Day[];
  at: string;
}

/** A time of the week on some days, such as a scheduled restart (D-138), in the site's zone. */
export function WeeklyTimeField(props: {
  value: WeeklyTime;
  onChange: (next: WeeklyTime) => void;
  error?: string | undefined;
  disabled?: boolean;
}) {
  return (
    <div class="flex flex-col gap-2">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-control border border-edge p-2.5">
        <Weekdays
          days={props.value.days}
          onChange={(days) => props.onChange({ ...props.value, days })}
          disabled={props.disabled}
        />
        <label class="flex items-center gap-2 text-fg-muted text-xs">
          {t.at}
          <input
            type="time"
            value={props.value.at}
            disabled={props.disabled}
            onInput={(event) => props.onChange({ ...props.value, at: event.currentTarget.value })}
            class={[INPUT, "h-8 w-28 border-edge"]}
          />
        </label>
      </div>
      <Show when={props.error}>
        <p class="text-crit text-xs" role="alert">
          {props.error}
        </p>
      </Show>
    </div>
  );
}
