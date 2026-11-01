import { Activity } from "lucide";
import { Show } from "solid-js";
import type { Resolution } from "../../api/gen/Resolution";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";
import { RangePicker } from "./RangePicker";
import type { Preset } from "./ranges";

/**
 * Above a set of charts: what the points are (live samples, or the resolution read), a link to
 * the same series in the Metrics explorer, and the range.
 */
export function ChartToolbar(props: {
  range: Preset | undefined;
  onRange: (preset: Preset) => void;
  options?: readonly Preset[];
  /** `live` for samples kept in memory, else the resolution of stored series. */
  source?: Resolution | "live" | undefined;
  /** The explorer's link for these series. */
  explore?: string | undefined;
}) {
  return (
    <div class="flex flex-wrap items-center justify-end gap-x-4 gap-y-2 text-fg-subtle text-xs">
      <Show when={props.source}>
        {(source) => (
          <span>
            {source() === "live"
              ? strings.chart.live
              : strings.chart.resolution[source() as Resolution]}
          </span>
        )}
      </Show>
      <Show when={props.explore}>
        {(href) => (
          <a
            href={href()}
            class="inline-flex items-center gap-1 rounded-control font-medium text-accent hover:underline"
          >
            <Icon icon={Activity} class="size-3.5" />
            {strings.metrics.open}
          </a>
        )}
      </Show>
      <RangePicker value={props.range} onChange={props.onRange} options={props.options} />
    </div>
  );
}

/** The explorer's link for an entity's series over a preset range. */
export function exploreHref(entity: string, range?: Preset): string {
  const params = new URLSearchParams({ entity, ...(range && { range }) });
  return `/insights/metrics?${params}`;
}
