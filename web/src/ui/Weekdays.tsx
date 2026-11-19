import { For } from "solid-js";
import { strings } from "../lib/strings";

const t = strings.schedule;

/** The days of the week, Monday first, as the API names them (`Weekday`). */
export const WEEK = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] as const;
export type Day = (typeof WEEK)[number];

/** A toggle button in an editor, such as a day or a band: pressed is on. */
export const TOGGLE =
  "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-control border px-2.5 text-xs font-medium transition-colors focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-default disabled:opacity-60";
export const toggled = (on: boolean) =>
  on ? "border-accent bg-accent/15 text-fg" : "border-edge text-fg-muted hover:bg-raised";

/** The days of the week as toggles, Monday first; each is named in full for screen readers. */
export function Weekdays(props: {
  days: readonly Day[];
  onChange: (days: Day[]) => void;
  disabled?: boolean;
}) {
  const toggle = (day: Day) =>
    props.onChange(
      props.days.includes(day) ? props.days.filter((d) => d !== day) : [...props.days, day],
    );
  return (
    <div class="flex flex-wrap gap-1">
      <For each={WEEK}>
        {(day) => (
          <button
            type="button"
            aria-pressed={props.days.includes(day) ? "true" : "false"}
            aria-label={t.dayNames[day]}
            class={[TOGGLE, "px-2", toggled(props.days.includes(day))]}
            disabled={props.disabled}
            onClick={() => toggle(day)}
          >
            {t.days[day]}
          </button>
        )}
      </For>
    </div>
  );
}
