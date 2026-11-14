import { For, Show } from "solid-js";
import type { Wall } from "../../../api/gen/Wall";
import type { PlanFrame, PlanPoint } from "../../../viz/floorplan/PlanCanvas";
import type { WallsState } from "./createWalls";
import { look, MATERIALS } from "./materials";

/** Snapping reaches this far on screen, at any zoom. */
export const SNAP_PX = 10;

/**
 * The floor's walls over the plan, in screen space so they keep their width at any zoom: one
 * path per material, however many walls. While drawing (`state`), the wall selected with its
 * draggable ends, the line being drawn with its length (`length` formats plan pixels) and a mark
 * where the pointer snaps.
 */
export function WallsLayer(props: {
  frame: PlanFrame;
  walls: readonly Wall[];
  state?: WallsState;
  length?: (px: number) => string | undefined;
}) {
  const path = (walls: readonly Wall[]) => {
    let d = "";
    for (const wall of walls) {
      const a = props.frame.project(wall.from);
      const b = props.frame.project(wall.to);
      d += `M${a.x.toFixed(1)} ${a.y.toFixed(1)}L${b.x.toFixed(1)} ${b.y.toFixed(1)}`;
    }
    return d;
  };
  const of = (material: Wall["material"]) =>
    path(props.walls.filter((w) => w.material === material));
  const selected = () => {
    const i = props.state?.selected();
    return i === undefined ? undefined : props.walls[i];
  };
  const tolerance = () => SNAP_PX / props.frame.zoom();
  const grab = (event: PointerEvent, end: "from" | "to") => {
    // The canvas pans on the background only.
    event.stopPropagation();
    if (event.button !== 0) return;
    (event.currentTarget as Element).setPointerCapture?.(event.pointerId);
    props.state?.grab(end);
  };
  const pull = (event: PointerEvent) => {
    if (!props.state?.dragging()) return;
    props.state.pull(props.frame.toPlan(event.clientX, event.clientY), tolerance(), event.altKey);
  };
  const label = (a: PlanPoint, b: PlanPoint) => props.length?.(Math.hypot(b.x - a.x, b.y - a.y));

  return (
    <g data-walls="">
      <g class="pointer-events-none" stroke-linecap="round" fill="none">
        <Show when={selected()}>
          {(wall) => (
            <path
              d={path([wall()])}
              class="stroke-accent"
              stroke-width="10"
              stroke-opacity="0.35"
            />
          )}
        </Show>
        <For each={MATERIALS}>
          {(m) => (
            <Show when={of(m.material)}>
              {(d) => (
                <path d={d()} stroke={m.color} stroke-width={m.width} data-material={m.material} />
              )}
            </Show>
          )}
        </For>
      </g>
      <Show when={props.state?.chain()}>
        {(from) => {
          const a = () => props.frame.project(from());
          const to = () => props.state?.hover()?.point;
          const b = () => {
            const at = to();
            return at ? props.frame.project(at) : a();
          };
          const text = () => {
            const at = to();
            return at ? label(from(), at) : undefined;
          };
          return (
            <g class="pointer-events-none">
              <line
                x1={a().x}
                y1={a().y}
                x2={b().x}
                y2={b().y}
                stroke={look(props.state?.material() ?? "drywall").color}
                stroke-width="3"
                stroke-dasharray="6 4"
                stroke-linecap="round"
              />
              <circle cx={a().x} cy={a().y} r="4" class="fill-accent" />
              <Show
                when={props.state?.hover()?.kind === "end" || props.state?.hover()?.kind === "line"}
              >
                <rect
                  x={b().x - 5}
                  y={b().y - 5}
                  width="10"
                  height="10"
                  class="stroke-accent"
                  fill="#fff"
                  stroke-width="2"
                />
              </Show>
              <Show when={text()}>
                {(value) => (
                  <text
                    x={(a().x + b().x) / 2}
                    y={(a().y + b().y) / 2 - 8}
                    text-anchor="middle"
                    fill="#111827"
                    stroke="#fff"
                    stroke-width="3"
                    paint-order="stroke"
                    class="font-medium font-mono text-xs"
                  >
                    {value()}
                  </text>
                )}
              </Show>
            </g>
          );
        }}
      </Show>
      <Show when={props.state && selected()}>
        {(wall) => (
          <For each={["from", "to"] as const}>
            {(end) => {
              const at = () => props.frame.project(wall()[end]);
              return (
                <circle
                  cx={at().x}
                  cy={at().y}
                  r="6"
                  fill="#fff"
                  class="cursor-move stroke-accent"
                  stroke-width="2.5"
                  data-end={end}
                  onPointerDown={(event) => grab(event, end)}
                  onPointerMove={pull}
                  onPointerUp={(event) => {
                    event.stopPropagation();
                    props.state?.release();
                  }}
                />
              );
            }}
          </For>
        )}
      </Show>
    </g>
  );
}
