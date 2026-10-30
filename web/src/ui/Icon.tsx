import { Dynamic, type JSX } from "@solidjs/web";
import type { IconNode } from "lucide";
import { For, omit } from "solid-js";

export type { IconNode };

export interface IconProps extends JSX.SvgSVGAttributes<SVGSVGElement> {
  /** An icon from the `lucide` package, e.g. `import { Router } from "lucide"`. */
  icon: IconNode;
}

/**
 * A Lucide icon. lucide-solid has no Solid 2 build (D-045), so this renders the framework-free
 * icon data from `lucide`, which keeps imports tree-shaken per icon. Decorative by default: pass
 * `aria-hidden={false}` with an `aria-label` when the icon is the only label.
 */
export function Icon(props: IconProps) {
  const others = omit(props, "icon");
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      {...others}
    >
      <For each={props.icon}>{([tag, attrs]) => <Dynamic component={tag} {...attrs} />}</For>
    </svg>
  );
}
