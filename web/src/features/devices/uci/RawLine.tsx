import { For, Show } from "solid-js";
import type { RawSection } from "../../../api/gen/RawSection";
import { strings } from "../../../lib/strings";
import { Badge, type Tone } from "../../../ui/Badge";
import { rawKind, rawTarget, rawText } from "./uci";

const t = strings.device.uci;
const tones: Record<ReturnType<typeof rawKind>, Tone> = {
  whole: "info",
  merged: "neutral",
  removed: "warn",
};

/** One raw section in a line: what it does, where, the options it sets and deletes. */
export function RawLine(props: { raw: RawSection }) {
  const kind = () => rawKind(props.raw);
  const matching = () => Object.entries(props.raw.matching);
  return (
    <div class="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1 text-xs">
      <Badge tone={tones[kind()]}>{t.kind[kind()]}</Badge>
      <span class="font-mono">{rawTarget(props.raw)}</span>
      <Show when={matching().length > 0}>
        <span class="text-fg-subtle">{t.where}</span>
        <span class="font-mono text-fg-muted">
          {matching()
            .map(([name, value]) => `${name}=${rawText(value)}`)
            .join(", ")}
        </span>
      </Show>
      <For each={Object.entries(props.raw.options)}>
        {([name, value]) => (
          <span class="font-mono">
            <span class="text-fg-muted">{name}</span>={rawText(value)}
          </span>
        )}
      </For>
      <For each={props.raw.unset}>
        {(name) => (
          <span class="font-mono text-crit">
            {t.deletes} {name}
          </span>
        )}
      </For>
    </div>
  );
}
