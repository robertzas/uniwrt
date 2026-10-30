import { createUniqueId, Show } from "solid-js";

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** The visible label; with `hideLabel` it is for screen readers only. */
  label: string;
  hideLabel?: boolean;
  /** Guidance under the label. */
  hint?: string | undefined;
  disabled?: boolean;
  class?: string;
}

/** A native checkbox in the accent color, with its label and an optional hint. */
export function Checkbox(props: CheckboxProps) {
  const id = createUniqueId();
  return (
    <div class={["flex items-start gap-2.5", props.class]}>
      <input
        id={id}
        type="checkbox"
        checked={props.checked}
        disabled={props.disabled}
        onChange={(event) => props.onChange(event.currentTarget.checked)}
        aria-describedby={props.hint ? `${id}-hint` : undefined}
        class="mt-0.5 size-4 shrink-0 cursor-pointer rounded accent-(--color-accent-fill) disabled:cursor-not-allowed disabled:opacity-50"
      />
      <div class={["min-w-0", { "sr-only": props.hideLabel }]}>
        <label
          for={id}
          class={["text-fg text-sm", props.disabled ? "opacity-60" : "cursor-pointer"]}
        >
          {props.label}
        </label>
        <Show when={props.hint}>
          {(hint) => (
            <p id={`${id}-hint`} class="mt-0.5 text-fg-subtle text-xs">
              {hint()}
            </p>
          )}
        </Show>
      </div>
    </div>
  );
}
