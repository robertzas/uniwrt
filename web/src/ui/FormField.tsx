/** A labelled form control and the native inputs' classes, shared by the forms. */
import type { JSX } from "@solidjs/web";

/** A native select in the toolbar's style. */
export const SELECT =
  "h-9 w-full cursor-pointer rounded-control border border-edge bg-canvas pr-7 pl-2.5 text-fg text-sm focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-60";

/** A text or number input in the forms' style. */
export const INPUT =
  "h-9 w-full rounded-control border bg-canvas px-2.5 text-fg text-sm outline-none transition-[border-color,box-shadow] placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30 disabled:opacity-60";

/** A labelled control in a form: the label above, a hint or an error below. */
export function FormField(props: {
  label: string;
  for?: string;
  hint?: string | undefined;
  error?: string | undefined;
  children: JSX.Element;
  class?: string;
}) {
  return (
    <div class={["flex min-w-0 flex-col gap-1.5", props.class]}>
      <label for={props.for} class="font-medium text-fg text-sm">
        {props.label}
      </label>
      {props.children}
      <p
        class={["text-xs", props.error ? "text-crit" : "text-fg-subtle"]}
        hidden={!props.error && !props.hint}
      >
        {props.error ?? props.hint}
      </p>
    </div>
  );
}
