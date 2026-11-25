import type { JSX } from "@solidjs/web";
import { For, Show } from "solid-js";
import { Icon, type IconNode } from "./Icon";

/** A next step an empty state offers: a link, or an action on the page. */
export type EmptyAction = { label: string; href: string } | { label: string; onClick: () => void };

export interface EmptyStateProps {
  icon: IconNode;
  title: string;
  description: string;
  /**
   * Its next steps (D-148): the first is the primary one, the rest are links beside it. Pass
   * none to a viewer who can't take them.
   */
  actions?: readonly EmptyAction[];
  children?: JSX.Element;
}

const PRIMARY =
  "inline-flex h-8 items-center gap-2 rounded-control bg-accent-fill px-3 font-medium text-on-accent text-sm hover:brightness-110";
const SECONDARY = "font-medium text-accent text-sm hover:underline";

/** A centered placeholder for a page or panel with nothing to show yet, and what to do about it. */
export function EmptyState(props: EmptyStateProps) {
  return (
    <section class="mx-auto flex max-w-md flex-col items-center py-20 text-center md:py-28">
      <div class="mb-5 grid size-12 place-items-center rounded-xl border border-edge bg-surface text-fg-muted shadow-sm">
        <Icon icon={props.icon} class="size-5" />
      </div>
      <h1 class="font-semibold text-lg tracking-tight">{props.title}</h1>
      <p class="mt-2 text-fg-muted text-sm leading-relaxed">{props.description}</p>
      <Show when={(props.actions ?? []).length > 0}>
        <div class="mt-6 flex flex-wrap items-center justify-center gap-x-4 gap-y-3">
          <For each={props.actions}>
            {(action, index) => {
              const style = () => (index() === 0 ? PRIMARY : SECONDARY);
              return "href" in action ? (
                <a href={action.href} class={style()}>
                  {action.label}
                </a>
              ) : (
                <button type="button" class={style()} onClick={() => action.onClick()}>
                  {action.label}
                </button>
              );
            }}
          </For>
        </div>
      </Show>
      {props.children}
    </section>
  );
}
