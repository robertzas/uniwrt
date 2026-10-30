import type { JSX } from "@solidjs/web";
import { CircleAlert, Eye, EyeOff } from "lucide";
import { createSignal, createUniqueId, Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "./Icon";

export interface TextFieldProps {
  label: string;
  value: string;
  onInput: (value: string) => void;
  type?: "text" | "password";
  name?: string;
  autocomplete?: JSX.InputHTMLAttributes<HTMLInputElement>["autocomplete"];
  /** Guidance under the field, such as a length rule. */
  hint?: string;
  /** Marks the field invalid and replaces the hint. */
  error?: string | undefined;
  required?: boolean;
  disabled?: boolean;
  autofocus?: boolean;
  minlength?: number;
}

/** A labelled text input. Password fields get a button that shows what was typed. */
export function TextField(props: TextFieldProps) {
  const id = createUniqueId();
  const [revealed, setRevealed] = createSignal(false);
  const password = () => props.type === "password";
  const note = () => props.error ?? props.hint;

  return (
    <div class="flex flex-col gap-1.5">
      <label for={id} class="font-medium text-fg text-sm">
        {props.label}
      </label>
      <div class="relative">
        <input
          id={id}
          name={props.name}
          type={password() && !revealed() ? "password" : "text"}
          value={props.value}
          onInput={(event) => props.onInput(event.currentTarget.value)}
          autocomplete={props.autocomplete}
          autocapitalize="none"
          spellcheck={false}
          required={props.required}
          disabled={props.disabled}
          autofocus={props.autofocus}
          minlength={props.minlength}
          aria-invalid={props.error ? "true" : undefined}
          aria-describedby={note() ? `${id}-note` : undefined}
          class={[
            "h-10 w-full rounded-control border bg-canvas px-3 text-fg text-sm outline-none transition-[border-color,box-shadow] placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30 disabled:opacity-60",
            props.error ? "border-crit" : "border-edge",
            { "pr-10": password() },
          ]}
        />
        <Show when={password()}>
          <button
            type="button"
            onClick={() => {
              setRevealed((shown) => !shown);
            }}
            aria-label={revealed() ? strings.auth.hidePassword : strings.auth.showPassword}
            aria-pressed={revealed() ? "true" : "false"}
            class="absolute inset-y-0 right-0 grid w-10 place-items-center rounded-r-control text-fg-subtle hover:text-fg"
          >
            <Icon icon={revealed() ? EyeOff : Eye} class="size-4" />
          </button>
        </Show>
      </div>
      <Show when={note()}>
        {(text) => (
          <p
            id={`${id}-note`}
            class={["flex items-start gap-1.5 text-xs", props.error ? "text-fg" : "text-fg-subtle"]}
          >
            <Show when={props.error}>
              <Icon icon={CircleAlert} class="mt-px size-3.5 shrink-0 text-crit" />
            </Show>
            {text()}
          </p>
        )}
      </Show>
    </div>
  );
}
