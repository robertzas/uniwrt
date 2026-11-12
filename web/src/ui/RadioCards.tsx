import { createUniqueId, For } from "solid-js";

export interface RadioCardOption<T extends string> {
  value: T;
  label: string;
  /** What choosing it does, under the label. */
  description: string;
}

/**
 * A choice among a few modes, each a card with what it does: native radios (arrow keys move
 * between them) inside a fieldset named by `label`, the chosen card in the accent color.
 */
export function RadioCards<T extends string>(props: {
  label: string;
  value: T;
  options: readonly RadioCardOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  class?: string;
}) {
  const name = createUniqueId();
  return (
    <fieldset class={["grid gap-2 sm:grid-cols-2", props.class]} disabled={props.disabled}>
      <legend class="sr-only">{props.label}</legend>
      <For each={props.options}>
        {(option) => {
          const id = `${name}-${option.value}`;
          const chosen = () => props.value === option.value;
          return (
            <label
              for={id}
              class={[
                "flex items-start gap-2.5 rounded-control border px-3 py-2.5 transition-colors has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent",
                chosen() ? "border-accent bg-accent/10" : "border-edge",
                props.disabled ? "opacity-60" : "cursor-pointer hover:border-edge-strong",
              ]}
            >
              <input
                id={id}
                type="radio"
                name={name}
                value={option.value}
                checked={chosen()}
                onChange={() => props.onChange(option.value)}
                aria-labelledby={`${id}-label`}
                aria-describedby={`${id}-text`}
                class="mt-0.5 size-4 shrink-0 accent-(--color-accent-fill) disabled:cursor-not-allowed"
              />
              <span class="min-w-0">
                <span id={`${id}-label`} class="block font-medium text-fg text-sm">
                  {option.label}
                </span>
                <span id={`${id}-text`} class="mt-0.5 block text-fg-muted text-xs">
                  {option.description}
                </span>
              </span>
            </label>
          );
        }}
      </For>
    </fieldset>
  );
}
