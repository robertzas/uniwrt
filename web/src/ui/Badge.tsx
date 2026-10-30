import type { JSX } from "@solidjs/web";
import { Show } from "solid-js";
import { Icon, type IconNode } from "./Icon";

export type Tone = "ok" | "warn" | "crit" | "info" | "neutral";

/** Status colors reach only 3:1, so they color the dot or icon and the label stays fg. */
const tones: Record<Tone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  crit: "bg-crit",
  info: "bg-info",
  neutral: "bg-fg-subtle",
};

const iconTones: Record<Tone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  crit: "text-crit",
  info: "text-info",
  neutral: "text-fg-subtle",
};

/** A small round status marker. Decorative: always pair it with text. */
export function StatusDot(props: { tone: Tone; pulse?: boolean; class?: string }) {
  return (
    <span
      aria-hidden="true"
      class={[
        "inline-block size-2 shrink-0 rounded-full",
        tones[props.tone],
        { "animate-pulse": props.pulse },
        props.class,
      ]}
    />
  );
}

/** A status label: a dot (or an icon) in the tone's color beside fg text. */
export function Badge(props: { tone?: Tone; icon?: IconNode; children: JSX.Element }) {
  const tone = () => props.tone ?? "neutral";
  return (
    <span class="inline-flex h-[1.375rem] shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full border border-edge bg-raised px-2 font-medium text-fg text-xs">
      <Show when={props.icon} fallback={<StatusDot tone={tone()} class="size-1.5" />}>
        {(icon) => <Icon icon={icon()} class={["size-3", iconTones[tone()]]} />}
      </Show>
      {props.children}
    </span>
  );
}
