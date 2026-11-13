import type { JSX } from "@solidjs/web";
import { Ruler } from "lucide";
import { createEffect, createMemo, createSignal, onSettled, Show } from "solid-js";
import type { Floor } from "../../api/gen/Floor";
import type { LengthUnit } from "../../api/gen/LengthUnit";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { INPUT, SELECT } from "../../ui/FormField";
import { Icon } from "../../ui/Icon";
import { showToast } from "../../ui/Toast";
import { PlanCanvas, type PlanFrame, type PlanPoint } from "../../viz/floorplan/PlanCanvas";
import { type Marker, Markers } from "./Markers";
import { scaleBar, toMeters } from "./units";

const t = strings.floors;

/** What clicks on the plan do: pan (idle), place a device, or pick the scale's two ends. */
export type Mode =
  | { kind: "idle" }
  | { kind: "place"; node: number; name: string }
  | { kind: "scale"; from?: PlanPoint; to?: PlanPoint };

/** A nudge is saved once the arrow keys rest this long. */
const NUDGE_SAVE_MS = 400;
const MIN_SCALE_PX = 10;

/**
 * A floor's plan with its devices (D-117). Admins drag or nudge devices (saved when released),
 * place one picked in the devices panel (`mode` "place") and set the scale by clicking both ends
 * of a known length, then entering it in m or ft. Every save answers with the floor (`onSaved`).
 */
export function FloorEditor(props: {
  floor: Floor;
  nodes: readonly NodeSummary[];
  admin: boolean;
  mode: Mode;
  onMode: (mode: Mode) => void;
  onSaved: (floor: Floor) => void;
  /** Drawn on the plan (the heatmap), given the positions being dragged or nudged. */
  underlay?: (moving: ReadonlyMap<number, PlanPoint>) => JSX.Element;
  /** Drawn over the plan under the devices (a suggested spot). */
  marks?: (frame: PlanFrame) => JSX.Element;
}) {
  const [zoom, setZoom] = createSignal(1);
  /** Positions being dragged or nudged, ahead of the floor that saves them. */
  const [moving, setMoving] = createSignal<ReadonlyMap<number, PlanPoint>>(new Map());
  const [length, setLength] = createSignal("");
  const [unit, setUnit] = createSignal<LengthUnit>("m");
  const [scaleError, setScaleError] = createSignal<string>();
  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  const image = () => props.floor.image;
  const names = createMemo(() => new Map(props.nodes.map((n) => [n.id, n])));
  const markers = createMemo<Marker[]>(() =>
    props.floor.nodes.map((placement) => {
      const node = names().get(placement.node);
      return {
        node: placement.node,
        name: node?.name ?? strings.metrics.node(String(placement.node)),
        state: node?.status.state,
        at: moving().get(placement.node) ?? placement.at,
      };
    }),
  );
  const clamp = (point: PlanPoint): PlanPoint => {
    const plan = image();
    if (!plan) return point;
    return {
      x: Math.min(plan.width_px, Math.max(0, point.x)),
      y: Math.min(plan.height_px, Math.max(0, point.y)),
    };
  };
  const setMoved = (node: number, at: PlanPoint | undefined) =>
    setMoving((now) => {
      const next = new Map(now);
      if (at) next.set(node, at);
      else next.delete(node);
      return next;
    });
  const save = async (node: number, at: PlanPoint) => {
    try {
      props.onSaved(await api.floors.place(props.floor.id, node, clamp(at)));
    } catch (err) {
      showToast({ title: errorText(err) ?? t.dialog.failed, tone: "crit" });
    } finally {
      setMoved(node, undefined);
    }
  };
  const drop = (node: number) => {
    const at = moving().get(node);
    if (at) void save(node, at);
  };
  const nudge = (node: number, dx: number, dy: number) => {
    const from = moving().get(node) ?? props.floor.nodes.find((p) => p.node === node)?.at;
    if (!from) return;
    const at = clamp({ x: from.x + dx, y: from.y + dy });
    setMoved(node, at);
    clearTimeout(timers.get(node));
    timers.set(
      node,
      setTimeout(() => {
        timers.delete(node);
        void save(node, at);
      }, NUDGE_SAVE_MS),
    );
  };

  const pick = (point: PlanPoint) => {
    const mode = props.mode;
    const at = clamp(point);
    if (mode.kind === "place") {
      props.onMode({ kind: "idle" });
      void save(mode.node, at);
    } else if (mode.kind === "scale") {
      if (!mode.from || mode.to) {
        props.onMode({ kind: "scale", from: at });
        setScaleError(undefined);
      } else if (Math.hypot(at.x - mode.from.x, at.y - mode.from.y) < MIN_SCALE_PX) {
        setScaleError(t.scale.tooShort);
      } else {
        props.onMode({ kind: "scale", from: mode.from, to: at });
      }
    }
  };
  const saveScale = async (event: SubmitEvent) => {
    event.preventDefault();
    const mode = props.mode;
    const value = Number(length());
    const meters = toMeters(value, unit());
    if (mode.kind !== "scale" || !mode.from || !mode.to) return;
    if (!Number.isFinite(value) || meters < 0.1 || meters > 1000) {
      setScaleError(t.scale.invalid);
      return;
    }
    const { name, level, height_m } = props.floor;
    try {
      const floor = await api.floors.update(props.floor.id, {
        name,
        level,
        height_m,
        scale: { from: mode.from, to: mode.to, length_m: meters, unit: unit() },
      });
      props.onSaved(floor);
      props.onMode({ kind: "idle" });
      showToast({ title: t.scale.saved, tone: "ok" });
    } catch (err) {
      setScaleError(errorText(err) ?? t.dialog.failed);
    }
  };

  // Each scale session starts from the floor's unit, empty.
  createEffect(
    () => props.mode.kind === "scale",
    (scaling) => {
      if (!scaling) return;
      setLength("");
      setUnit(props.floor.scale?.unit ?? "m");
      setScaleError(undefined);
    },
  );
  onSettled(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && props.mode.kind !== "idle") props.onMode({ kind: "idle" });
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      for (const timer of timers.values()) clearTimeout(timer);
    };
  });

  const hint = () => {
    const mode = props.mode;
    if (mode.kind === "place") return t.devices.placeHint(mode.name);
    if (mode.kind === "scale" && !mode.to) return mode.from ? t.scale.pickTo : t.scale.pickFrom;
    return undefined;
  };
  const bar = () => {
    const scale = props.floor.scale;
    if (!scale) return undefined;
    const length_px = Math.hypot(scale.to.x - scale.from.x, scale.to.y - scale.from.y);
    return scaleBar(scale.length_m / length_px, zoom(), scale.unit);
  };

  return (
    <Show when={image()}>
      {(plan) => (
        <PlanCanvas
          src={api.floors.imageUrl(props.floor.id, plan().version)}
          width={plan().width_px}
          height={plan().height_px}
          label={t.canvas.plan(props.floor.name)}
          picking={props.mode.kind !== "idle"}
          onPick={pick}
          onZoom={setZoom}
          underlay={props.underlay?.(moving())}
          overlay={
            <>
              <Show when={hint()}>
                {(text) => (
                  <p
                    role="status"
                    class="absolute top-2 left-2 max-w-[70%] rounded-control border border-edge bg-surface/95 px-3 py-2 text-sm shadow-card"
                  >
                    {text()}
                    <Show when={scaleError()}>
                      {(error) => <span class="mt-1 block text-crit text-xs">{error()}</span>}
                    </Show>
                  </p>
                )}
              </Show>
              <Show when={props.mode.kind === "scale" && props.mode.to}>
                <form
                  aria-label={t.scale.set}
                  onSubmit={saveScale}
                  class="absolute top-2 left-2 flex max-w-[calc(100%-4rem)] flex-wrap items-end gap-2 rounded-control border border-edge bg-surface/95 p-3 shadow-card"
                >
                  <div class="flex flex-col gap-1 text-xs">
                    <label for="scale-length" class="font-medium">
                      {t.scale.length}
                    </label>
                    <input
                      id="scale-length"
                      class={[INPUT, "w-28"]}
                      type="number"
                      inputmode="decimal"
                      step="any"
                      min="0"
                      value={length()}
                      onInput={(event) => setLength(event.currentTarget.value)}
                      autofocus
                    />
                  </div>
                  <div class="flex flex-col gap-1 text-xs">
                    <label for="scale-unit" class="font-medium">
                      {t.scale.unit}
                    </label>
                    <select
                      id="scale-unit"
                      class={[SELECT, "w-20"]}
                      value={unit()}
                      onChange={(event) => setUnit(event.currentTarget.value as LengthUnit)}
                    >
                      <option value="m">m</option>
                      <option value="ft">ft</option>
                    </select>
                  </div>
                  <Button type="submit" variant="primary" size="sm">
                    {t.scale.save}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => props.onMode({ kind: "idle" })}
                  >
                    {t.scale.cancel}
                  </Button>
                  <Show when={scaleError()}>
                    {(error) => <p class="w-full text-crit text-xs">{error()}</p>}
                  </Show>
                </form>
              </Show>
              <Show when={bar()}>
                {(b) => (
                  <div
                    role="img"
                    aria-label={`${t.scale.bar}: ${b().label}`}
                    class="pointer-events-none absolute bottom-2 left-2 flex items-center gap-2 rounded-control bg-surface/90 px-2 py-1 text-fg text-xs shadow-card"
                  >
                    <Icon icon={Ruler} class="size-3.5 text-fg-muted" />
                    <svg width={Math.round(b().px)} height="8" aria-hidden="true">
                      <path
                        d={`M0.5 0v8M${Math.round(b().px) - 0.5} 0v8M0 4H${Math.round(b().px)}`}
                        class="stroke-fg"
                        stroke-width="1.5"
                      />
                    </svg>
                    <span class="font-mono tabular-nums">{b().label}</span>
                  </div>
                )}
              </Show>
            </>
          }
        >
          {(frame) => (
            <>
              <Show when={props.mode.kind === "scale" && props.mode.from}>
                {(from) => {
                  const a = () => frame.project(from());
                  const b = () => {
                    const mode = props.mode;
                    return mode.kind === "scale" && mode.to ? frame.project(mode.to) : a();
                  };
                  return (
                    <g class="pointer-events-none">
                      <line
                        x1={a().x}
                        y1={a().y}
                        x2={b().x}
                        y2={b().y}
                        class="stroke-accent"
                        stroke-width="2.5"
                        stroke-dasharray="6 4"
                      />
                      <circle cx={a().x} cy={a().y} r="5" class="fill-accent" />
                      <circle cx={b().x} cy={b().y} r="5" class="fill-accent" />
                    </g>
                  );
                }}
              </Show>
              {props.marks?.(frame)}
              <Markers
                frame={frame}
                markers={markers()}
                editable={props.admin && props.mode.kind === "idle"}
                onMove={(node, at) => setMoved(node, clamp(at))}
                onDrop={drop}
                onNudge={nudge}
              />
            </>
          )}
        </PlanCanvas>
      )}
    </Show>
  );
}
