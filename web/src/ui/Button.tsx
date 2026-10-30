import type { JSX } from "@solidjs/web";
import { omit, Show } from "solid-js";
import { Icon, type IconNode } from "./Icon";

export type ButtonVariant = "primary" | "secondary" | "ghost";
export type ButtonSize = "sm" | "md";

export interface ButtonProps extends JSX.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** A leading icon. Without children the button is square; give it an `aria-label`. */
  icon?: IconNode;
}

const variants: Record<ButtonVariant, string> = {
  primary:
    "bg-accent-fill text-on-accent hover:brightness-110 glass:shadow-[0_0_20px_-6px_var(--color-accent-fill)]",
  secondary: "border border-edge bg-surface text-fg hover:bg-raised",
  ghost: "text-fg-muted hover:bg-raised hover:text-fg",
};

const sizes: Record<ButtonSize, { text: string; square: string; icon: string }> = {
  sm: { text: "h-7 gap-1.5 px-2.5 text-xs", square: "size-7", icon: "size-3.5" },
  md: { text: "h-8 gap-2 px-3 text-sm", square: "size-8", icon: "size-4" },
};

/** A button. `type` defaults to "button", so it never submits a form by accident. */
export function Button(props: ButtonProps) {
  const others = omit(props, "variant", "size", "icon", "class", "children");
  const size = () => sizes[props.size ?? "md"];
  return (
    <button
      type="button"
      class={[
        "inline-flex shrink-0 items-center justify-center rounded-control font-medium transition-[background-color,color,filter] disabled:pointer-events-none disabled:opacity-50",
        variants[props.variant ?? "secondary"],
        props.children === undefined ? size().square : size().text,
        props.class,
      ]}
      {...others}
    >
      <Show when={props.icon}>{(icon) => <Icon icon={icon()} class={size().icon} />}</Show>
      {props.children}
    </button>
  );
}
