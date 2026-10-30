import type { JSX } from "@solidjs/web";
import { Show } from "solid-js";

/** A panel on the canvas. Its border, radius, shadow and (glass) blur come from the skin. */
export function Card(props: { class?: string; children?: JSX.Element; "aria-label"?: string }) {
  return (
    <section
      aria-label={props["aria-label"]}
      class={[
        "rounded-card border border-card-edge bg-surface shadow-card glass:backdrop-blur-panel",
        props.class,
      ]}
    >
      {props.children}
    </section>
  );
}

/** A card's title row: a heading, an optional description and actions on the right. */
export function CardHeader(props: { title: string; description?: string; actions?: JSX.Element }) {
  return (
    <header class="flex items-start gap-3 px-4 pt-3.5 pb-2">
      <div class="min-w-0 flex-1">
        <h2 class="font-semibold text-sm tracking-tight">{props.title}</h2>
        <Show when={props.description}>
          {(description) => <p class="mt-0.5 text-fg-subtle text-xs">{description()}</p>}
        </Show>
      </div>
      <Show when={props.actions}>
        <div class="flex shrink-0 items-center gap-2 text-fg-subtle text-xs">{props.actions}</div>
      </Show>
    </header>
  );
}
