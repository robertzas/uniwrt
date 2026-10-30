import type { JSX } from "@solidjs/web";
import { Show } from "solid-js";
import { Card } from "./Card";

export interface StatProps {
  label: string;
  value: JSX.Element;
  unit?: string;
  /** A line under the value: context, a breakdown or a trend. */
  children?: JSX.Element;
}

/** A headline number in a card: label, value with its unit, and optional detail. */
export function Stat(props: StatProps) {
  return (
    <Card class="flex min-w-0 flex-col gap-0.5 px-3.5 py-3">
      <span class="truncate font-medium text-fg-subtle text-xs">{props.label}</span>
      <span class="truncate font-semibold text-2xl tabular-nums leading-tight tracking-tight">
        {props.value}
        <Show when={props.unit}>
          <span class="ml-1 font-medium text-fg-subtle text-sm tracking-normal">{props.unit}</span>
        </Show>
      </span>
      <Show when={props.children}>
        <div class="mt-1 min-w-0 text-fg-subtle text-xs">{props.children}</div>
      </Show>
    </Card>
  );
}
