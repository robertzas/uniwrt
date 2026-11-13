import { useQuery } from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal } from "solid-js";
import type { Band } from "../../../api/gen/Band";
import type { BandModel } from "../../../api/gen/BandModel";
import type { Floor } from "../../../api/gen/Floor";
import { api } from "../../../lib/api";
import type { HeatGrid } from "../../../viz/floorplan/Heatmap";
import type { PlanPoint } from "../../../viz/floorplan/PlanCanvas";
import { loadFootprint } from "./footprint";
import {
  deadZones,
  type Grid,
  grid,
  type Mask,
  metersPerPx,
  type Suggestion,
  shape,
  sources,
  suggest,
} from "./model";

export const COVERAGE_KEY = ["floors", "coverage"];
/** The model is fetched again this often while coverage shows (the matrix changes slowly). */
const REFRESH_MS = 60_000;
/** Openings up to twice this wide (doorways, garage and patio doors) close when finding the footprint. */
const CLOSE_M = 1.5;

/** What the floor's coverage comes to with the devices where they're saved. */
export interface Summary {
  area: number;
  dead: number;
  suggestion: Suggestion | undefined;
  /** The band's transmitters not placed on a scaled floor. */
  unplaced: number;
  /** Placed transmitters. */
  placed: number;
}

/**
 * The coverage of `floor` on `band` (undefined: off; D-118): the controller's fitted models,
 * the plan's footprint once read, the heatmap for any positions being dragged, and the dead
 * zones and suggested spot for the saved ones.
 */
export function createCoverage(props: {
  floor: () => Floor | undefined;
  floors: () => readonly Floor[];
  band: () => Band | undefined;
}) {
  const query = useQuery(() => ({
    queryKey: COVERAGE_KEY,
    queryFn: ({ signal }) => api.floors.coverage(signal),
    enabled: props.band() !== undefined,
    refetchInterval: REFRESH_MS,
  }));
  const bands = (): Band[] => (query.isSuccess ? query.data.bands.map((b) => b.band) : []);
  const model = (): BandModel | undefined =>
    query.isSuccess ? query.data.bands.find((b) => b.band === props.band()) : undefined;

  const [mask, setMask] = createSignal<{ key: string; mask: Mask }>();
  /** What the footprint is read from: the plan's URL and the grid's shape. */
  const footprintKey = () => {
    const floor = props.floor();
    const per = floor && metersPerPx(floor);
    if (props.band() === undefined || !floor?.image || !per) return undefined;
    const { cols, rows, cellPx } = shape(floor.image.width_px, floor.image.height_px);
    const close = Math.min(12, Math.max(1, Math.round(CLOSE_M / (cellPx * per))));
    const src = api.floors.imageUrl(floor.id, floor.image.version);
    return { key: `${src}|${cols}|${rows}|${close}`, src, cols, rows, close };
  };
  createEffect(
    () => footprintKey(),
    (wanted) => {
      if (!wanted || mask()?.key === wanted.key) return;
      let live = true;
      void loadFootprint(wanted.src, wanted.cols, wanted.rows, wanted.close).then((found) => {
        if (live) setMask({ key: wanted.key, mask: found });
      });
      return () => {
        live = false;
      };
    },
  );
  const footprint = (): Mask | undefined => {
    const wanted = footprintKey();
    const held = mask();
    return wanted && held?.key === wanted.key ? held.mask : undefined;
  };
  const fits = (g: Grid, m: Mask | undefined) =>
    m && m.cols === g.cols && m.rows === g.rows ? m : undefined;

  /** The heatmap with the devices in `moving` (on this floor) where they're being dragged. */
  const heat = (moving: ReadonlyMap<number, PlanPoint>): (HeatGrid & Grid) | undefined => {
    const m = model();
    const floor = props.floor();
    if (!m || !floor) return undefined;
    const list = sources(m, props.floors(), { floor: floor.id, at: moving });
    if (list.length === 0) return undefined;
    const g = grid(m, list, floor, props.floors());
    if (!g) return undefined;
    return { ...g, values: g.dbm, mask: fits(g, footprint())?.inside };
  };

  const summary = createMemo((): Summary | undefined => {
    const m = model();
    const floor = props.floor();
    const per = floor && metersPerPx(floor);
    if (!m || !floor || !per) return undefined;
    const floors = props.floors();
    const list = sources(m, floors);
    const g = grid(m, list, floor, floors);
    if (!g) return undefined;
    const placed = new Set(list.map((s) => s.node));
    const unplaced = m.transmitters.filter((t) => !placed.has(t.node)).length;
    const m5 = query.isSuccess ? query.data.bands.find((b) => b.band === "5g") : undefined;
    const mesh = m5 ? sources(m5, floors) : [];
    const area = fits(g, footprint());
    const zones = deadZones(g, area, per);
    const suggestion =
      zones.dead > 0 && list.length > 0
        ? suggest(
            m,
            list,
            floor,
            floors,
            g,
            area,
            m5 && mesh.length ? { model: m5, sources: mesh } : undefined,
          )
        : undefined;
    return { ...zones, suggestion, unplaced, placed: list.length };
  });

  return { query, bands, model, footprint, heat, summary };
}

export type CoverageState = ReturnType<typeof createCoverage>;
