import { For } from "solid-js";
import { strings } from "../../lib/strings";

const l = strings.topology.legend;
const ramp = ["bg-link-1", "bg-link-2", "bg-link-3", "bg-link-4", "bg-link-5"] as const;
const bands = [
  ["bg-band-2g", strings.topology.band["2g"]],
  ["bg-band-5g", strings.topology.band["5g"]],
  ["bg-band-6g", strings.topology.band["6g"]],
] as const;

/** What the map's colors, lines and rings mean. */
export function Legend() {
  return (
    <section
      aria-label={strings.topology.toolbar.legend}
      class="absolute bottom-16 left-3 flex w-60 flex-col gap-3 rounded-card border border-card-edge bg-surface/95 p-3 text-xs shadow-overlay glass:backdrop-blur-panel"
    >
      <div>
        <p class="mb-1.5 font-medium text-fg-muted">{l.quality}</p>
        <div class="flex h-1.5 overflow-hidden rounded-full">
          <For each={ramp}>{(swatch) => <span class={["flex-1", swatch]} />}</For>
        </div>
        <div class="mt-1 flex justify-between text-fg-subtle">
          <span>{l.poor}</span>
          <span>{l.excellent}</span>
        </div>
      </div>
      <ul class="flex flex-col gap-1.5 text-fg-muted">
        <li class="flex items-center gap-2">
          <span class="h-0 w-6 border-edge-strong border-t-2" />
          {l.wired}
        </li>
        <li class="flex items-center gap-2">
          <span class="h-0 w-6 border-crit border-t-2 border-dashed" />
          {l.down}
        </li>
      </ul>
      <div>
        <p class="mb-1.5 font-medium text-fg-muted">{l.bands}</p>
        <ul class="flex flex-wrap gap-x-3 gap-y-1 text-fg-muted">
          <For each={bands}>
            {([swatch, label]) => (
              <li class="flex items-center gap-1.5">
                <span class={["size-2 rounded-full", swatch]} />
                {label}
              </li>
            )}
          </For>
        </ul>
      </div>
      <p class="text-fg-subtle">
        {l.health}. {l.flows}.
      </p>
    </section>
  );
}
