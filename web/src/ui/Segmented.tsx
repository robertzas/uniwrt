import { For } from "solid-js";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  /** A longer name for screen readers, when the label is terse (`24h` → "Last 24 hours"). */
  description?: string;
}

/**
 * A small group of toggle buttons, one of them pressed: ranges and modes in card headers. Each
 * option is a button with `aria-pressed`, inside a fieldset named by `label`.
 */
export function Segmented<T extends string>(props: {
  label: string;
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  class?: string;
  disabled?: boolean;
}) {
  return (
    <fieldset
      disabled={props.disabled}
      class={["flex rounded-control border border-edge p-0.5 disabled:opacity-60", props.class]}
    >
      <legend class="sr-only">{props.label}</legend>
      <For each={props.options}>
        {(option) => (
          <button
            type="button"
            aria-pressed={props.value === option.value ? "true" : "false"}
            aria-label={option.description}
            title={option.description}
            onClick={() => props.onChange(option.value)}
            class={[
              "h-7 rounded-control px-2.5 font-medium text-xs tabular-nums transition-colors focus-visible:outline-2 focus-visible:outline-accent",
              props.value === option.value ? "bg-raised text-fg" : "text-fg-subtle hover:text-fg",
            ]}
          >
            {option.label}
          </button>
        )}
      </For>
    </fieldset>
  );
}
