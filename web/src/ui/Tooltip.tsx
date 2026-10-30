import * as KTooltip from "@kobalte/core/tooltip";
import type { JSX } from "@solidjs/web";
import { omit } from "solid-js";

export interface TooltipProps extends JSX.HTMLAttributes<HTMLElement> {
  /** The hint. It supplements the trigger's own accessible name; it never replaces it. */
  content: string;
  placement?: "top" | "right" | "bottom" | "left";
  /** The trigger element: a button by default, or a link. */
  as?: "button" | "a";
  href?: string;
}

/** A trigger with a hint that appears on hover and keyboard focus. */
export function Tooltip(props: TooltipProps) {
  const others = omit(props, "content", "placement", "as", "children");
  return (
    <KTooltip.Root openDelay={350} closeDelay={0} placement={props.placement ?? "top"} gutter={6}>
      <KTooltip.Trigger as={props.as ?? "button"} {...others}>
        {props.children}
      </KTooltip.Trigger>
      <KTooltip.Portal>
        <KTooltip.Content class="z-50 max-w-xs rounded-control bg-fg px-2 py-1 font-medium text-canvas text-xs shadow-overlay data-expanded:animate-fade-in data-closed:animate-fade-out">
          {props.content}
        </KTooltip.Content>
      </KTooltip.Portal>
    </KTooltip.Root>
  );
}
