import { createMemo, For, Show } from "solid-js";
import type { NodeState } from "../../api/gen/NodeState";
import { strings } from "../../lib/strings";
import type { PlanFrame, PlanPoint } from "../../viz/floorplan/PlanCanvas";

const t = strings.floors.devices;

/** A device on the plan, where it is drawn now (dragged or nudged positions included). */
export interface Marker {
  node: number;
  name: string;
  state: NodeState | undefined;
  at: PlanPoint;
}

const tones: Record<NodeState, string> = {
  online: "fill-ok",
  degraded: "fill-warn",
  offline: "fill-crit",
};

/** Arrow keys move a marker this many screen pixels (Shift: five times). */
const NUDGE_PX = 8;

/**
 * The devices placed on the floor, in screen space so they keep their size at any zoom. Admins
 * drag them (`onMove` while dragging, `onDrop` once released) or focus one and use the arrow
 * keys (`onNudge`). Plans are drawn on white, so labels are dark with a white halo in any theme.
 */
export function Markers(props: {
  frame: PlanFrame;
  markers: readonly Marker[];
  editable: boolean;
  onMove: (node: number, at: PlanPoint) => void;
  onDrop: (node: number) => void;
  onNudge: (node: number, dx: number, dy: number) => void;
}) {
  let dragging: { node: number; pointer: number; moved: boolean } | undefined;
  const down = (event: PointerEvent, node: number) => {
    // The canvas pans on the background only.
    event.stopPropagation();
    if (!props.editable || event.button !== 0) return;
    dragging = { node, pointer: event.pointerId, moved: false };
    (event.currentTarget as Element).setPointerCapture?.(event.pointerId);
  };
  const move = (event: PointerEvent) => {
    if (dragging?.pointer !== event.pointerId) return;
    dragging.moved = true;
    props.onMove(dragging.node, props.frame.toPlan(event.clientX, event.clientY));
  };
  const up = (event: PointerEvent) => {
    if (dragging?.pointer !== event.pointerId) return;
    const { node, moved } = dragging;
    dragging = undefined;
    if (moved) props.onDrop(node);
  };
  const key = (event: KeyboardEvent, node: number) => {
    const step = (event.shiftKey ? 5 * NUDGE_PX : NUDGE_PX) / props.frame.zoom();
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = moves[event.key];
    if (!props.editable || !delta) return;
    event.preventDefault();
    props.onNudge(node, delta[0], delta[1]);
  };
  // Keyed by node id, so a marker keeps its element (and the pointer's capture) as it moves.
  const byNode = createMemo(() => new Map(props.markers.map((m) => [m.node, m])));
  const ids = createMemo(() => props.markers.map((m) => m.node));
  return (
    <For each={ids()}>
      {(id) => (
        <Show when={byNode().get(id)}>
          {(marker) => {
            const at = () => props.frame.project(marker().at);
            return (
              // biome-ignore lint/a11y/useSemanticElements: an SVG group can't be a <button>.
              <g
                role="button"
                tabindex={props.editable ? 0 : -1}
                aria-label={props.editable ? t.marker(marker().name) : marker().name}
                transform={`translate(${at().x} ${at().y})`}
                class={[
                  "outline-none [&:focus-visible>circle:first-child]:stroke-accent",
                  props.editable && "cursor-move",
                ]}
                onPointerDown={(event) => down(event, marker().node)}
                onPointerMove={move}
                onPointerUp={up}
                onKeyDown={(event) => key(event, marker().node)}
              >
                <circle r="13" fill="#fff" stroke="#fff" stroke-width="3" />
                <circle
                  r="9"
                  class={marker().state ? tones[marker().state as NodeState] : "fill-fg-subtle"}
                />
                <circle r="3.5" fill="#fff" />
                <text
                  y="27"
                  text-anchor="middle"
                  fill="#111827"
                  stroke="#fff"
                  stroke-width="3"
                  paint-order="stroke"
                  class="pointer-events-none font-medium text-xs"
                >
                  {marker().name}
                </text>
              </g>
            );
          }}
        </Show>
      )}
    </For>
  );
}
