import type { JSX } from "@solidjs/web";
import { Icon, type IconNode } from "./Icon";

export interface EmptyStateProps {
  icon: IconNode;
  title: string;
  description: string;
  children?: JSX.Element;
}

/** A centered placeholder for a page or panel with nothing to show yet. */
export function EmptyState(props: EmptyStateProps) {
  return (
    <section class="mx-auto flex max-w-md flex-col items-center py-20 text-center md:py-28">
      <div class="mb-5 grid size-12 place-items-center rounded-xl border border-edge bg-surface text-fg-muted shadow-sm">
        <Icon icon={props.icon} class="size-5" />
      </div>
      <h1 class="font-semibold text-lg tracking-tight">{props.title}</h1>
      <p class="mt-2 text-fg-muted text-sm leading-relaxed">{props.description}</p>
      {props.children}
    </section>
  );
}
