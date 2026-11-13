import { Show } from "solid-js";
import { strings } from "../../../lib/strings";
import { type HeatGrid, Heatmap, type Stop } from "../../../viz/floorplan/Heatmap";
import type { PlanFrame } from "../../../viz/floorplan/PlanCanvas";
import type { Grid, Suggestion } from "./model";

const t = strings.floors.coverage;

/** The signal's colors, in dBm: red below −75 (dead), amber to −67, green above (D-118). */
export const STOPS: readonly Stop[] = [
  { value: -85, rgb: [220, 38, 38] },
  { value: -75, rgb: [249, 115, 22] },
  { value: -71, rgb: [234, 179, 8] },
  { value: -67, rgb: [132, 204, 22] },
  { value: -50, rgb: [22, 163, 74] },
];

/** The heatmap on the plan: the grid covers whole cells, so it may reach past the plan's edge. */
export function CoverageLayer(props: { grid: HeatGrid & Grid }) {
  return (
    <Heatmap
      grid={props.grid}
      stops={STOPS}
      width={props.grid.cols * props.grid.cellPx}
      height={props.grid.rows * props.grid.cellPx}
    />
  );
}

/** Where another node would help most, in screen space under the devices. */
export function SuggestedMark(props: { frame: PlanFrame; suggestion: Suggestion | undefined }) {
  return (
    <Show when={props.suggestion}>
      {(s) => {
        const at = () => props.frame.project(s().at);
        return (
          <g
            class="pointer-events-none"
            transform={`translate(${at().x} ${at().y})`}
            data-suggested=""
          >
            <title>{t.suggested}</title>
            <circle
              r="16"
              class="stroke-accent"
              fill="#fff"
              fill-opacity="0.85"
              stroke-width="2.5"
              stroke-dasharray="5 3"
            />
            <path
              d="M-7 0H7M0 -7V7"
              class="stroke-accent"
              stroke-width="2.5"
              stroke-linecap="round"
            />
          </g>
        );
      }}
    </Show>
  );
}
