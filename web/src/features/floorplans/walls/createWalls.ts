import { createEffect, createSignal } from "solid-js";
import type { Floor } from "../../../api/gen/Floor";
import type { Material } from "../../../api/gen/Material";
import type { Wall } from "../../../api/gen/Wall";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { showToast } from "../../../ui/Toast";
import type { PlanPoint } from "../../../viz/floorplan/PlanCanvas";
import { type End, endsAt, hitWall, moveEnds, nearestEnd, type Snapped, snap } from "./snap";

const t = strings.floors.walls;

/**
 * Plain copies of `walls`: the query's data is reconciled in place when an answer arrives
 * (D-047), so a list kept for undo must not share its arrays or objects.
 */
const copy = (walls: readonly Wall[]): Wall[] =>
  walls.map((w) => ({ from: { ...w.from }, to: { ...w.to }, material: w.material }));
/** Undo keeps this many steps per floor shown. */
const UNDO_STEPS = 50;
/** A click this soon after a wall ended on the same spot is the double-click's second half. */
const DOUBLE_CLICK_MS = 500;

/**
 * The walls of the floor shown and what drawing them is doing (M6.2c): the polyline being drawn,
 * the wall selected, an end being dragged. Every edit shows at once (the heatmap follows the
 * drawn walls) and saves the whole list in the background, one request at a time, the latest
 * list winning; a failed save goes back to the controller's walls.
 */
export function createWalls(props: {
  floor: () => Floor | undefined;
  onSaved: (floor: Floor) => void;
}) {
  const [draft, setDraft] = createSignal<readonly Wall[]>();
  const [history, setHistory] = createSignal<readonly (readonly Wall[])[]>([]);
  const [material, setMaterial] = createSignal<Material>("drywall");
  /** The corner drawn last, while drawing. */
  const [chain, setChain] = createSignal<PlanPoint>();
  const [selected, setSelected] = createSignal<number>();
  /** Where the pointer is, snapped, while drawing. */
  const [hover, setHover] = createSignal<Snapped>();
  /** The ends being dragged and where to. */
  const [drag, setDrag] = createSignal<{ ends: End[]; to: PlanPoint }>();

  const saved = (): readonly Wall[] => props.floor()?.walls ?? [];
  const walls = (): readonly Wall[] => draft() ?? saved();
  /** The walls as drawn now: an end being dragged where it is. */
  const shown = (): readonly Wall[] => {
    const d = drag();
    return d ? moveEnds(walls(), d.ends, d.to, true) : walls();
  };

  // The save queue keeps the list to send outside the signal: Solid applies writes when it
  // flushes, so a read right after `setDraft` would still see the previous list.
  let latest: readonly Wall[] | undefined;
  let inflight = false;
  let again = false;
  const flush = async (): Promise<void> => {
    if (inflight) {
      again = true;
      return;
    }
    const floor = props.floor();
    const sent = latest;
    if (!floor || !sent) return;
    inflight = true;
    try {
      const answer = await api.floors.walls(floor.id, sent);
      props.onSaved(answer);
      if (latest === sent) {
        latest = undefined;
        setDraft(undefined);
      }
    } catch (err) {
      showToast({ title: errorText(err) ?? t.failed, tone: "crit" });
      latest = undefined;
      setDraft(undefined);
      setSelected(undefined);
    } finally {
      inflight = false;
      if (again) {
        again = false;
        void flush();
      }
    }
  };
  /** The walls now, with an edit that hasn't flushed yet. */
  const current = (): readonly Wall[] => latest ?? walls();
  const commit = (next: readonly Wall[]) => {
    // Read before the write: an updater may run when Solid flushes, after `latest` moved on.
    const before = copy(current());
    setHistory((h) => [...h.slice(-(UNDO_STEPS - 1)), before]);
    latest = copy(next);
    setDraft(latest);
    void flush();
  };
  const undo = () => {
    const h = history();
    const last = h[h.length - 1];
    if (!last) return;
    setHistory(h.slice(0, -1));
    setSelected(undefined);
    setChain(undefined);
    latest = last;
    setDraft(last);
    void flush();
  };

  // Another floor starts afresh.
  createEffect(
    () => props.floor()?.id,
    () => {
      latest = undefined;
      setDraft(undefined);
      setHistory([]);
      end();
    },
  );

  function end() {
    setChain(undefined);
    setSelected(undefined);
    setHover(undefined);
    setDrag(undefined);
  }

  /** Where and when the last wall ended with a second click on its corner. */
  let ended: { at: PlanPoint; time: number } | undefined;
  /** A click on the plan at `point`; `tolerance` is the snapping distance in plan pixels. */
  const click = (point: PlanPoint, tolerance: number, free: boolean, now = performance.now()) => {
    const list = walls();
    const from = chain();
    const near = (a: PlanPoint, b: PlanPoint) =>
      Math.hypot(a.x - b.x, a.y - b.y) < Math.max(1, tolerance / 2);
    if (ended && now - ended.time < DOUBLE_CLICK_MS && near(point, ended.at)) {
      // The rest of a double-click that ended the wall: it doesn't start another.
      ended = undefined;
      return;
    }
    ended = undefined;
    if (from) {
      const to = snap(point, from, list, tolerance, free).point;
      if (near(to, from)) {
        // A second click on the same corner (a double-click) ends the wall.
        setChain(undefined);
        setHover(undefined);
        ended = { at: point, time: now };
        return;
      }
      commit([...list, { from, to, material: material() }]);
      setChain(to);
      return;
    }
    const corner = free ? undefined : nearestEnd(point, list, tolerance);
    if (corner) {
      setSelected(undefined);
      setChain(corner);
      return;
    }
    const hit = hitWall(point, list, tolerance);
    if (hit !== undefined && hit !== selected()) {
      setSelected(hit);
      setMaterial((list[hit] as Wall).material);
      return;
    }
    setSelected(undefined);
    setChain(snap(point, undefined, list, tolerance, free).point);
  };
  const move = (point: PlanPoint | undefined, tolerance: number, free: boolean) => {
    const from = chain();
    setHover(point && from ? snap(point, from, walls(), tolerance, free) : undefined);
  };

  /** Starts dragging the selected wall's `end`, with every wall sharing that corner. */
  const grab = (end: "from" | "to") => {
    const i = selected();
    const wall = i === undefined ? undefined : walls()[i];
    if (!wall) return;
    setDrag({ ends: endsAt(wall[end], walls()), to: wall[end] });
  };
  const pull = (point: PlanPoint, tolerance: number, free: boolean) => {
    const d = drag();
    const i = selected();
    const wall = i === undefined ? undefined : walls()[i];
    if (!d || !wall) return;
    const moving = d.ends.find((e) => e.wall === i);
    const other = moving?.end === "from" ? wall.to : wall.from;
    setDrag({ ...d, to: snap(point, other, walls(), tolerance, free, d.ends).point });
  };
  const release = () => {
    const d = drag();
    if (!d) return;
    setDrag(undefined);
    const next = moveEnds(walls(), d.ends, d.to);
    if (next.length !== walls().length) setSelected(undefined);
    commit(next);
  };

  const remove = () => {
    const i = selected();
    if (i === undefined) return;
    setSelected(undefined);
    commit(walls().filter((_, j) => j !== i));
  };
  const pickMaterial = (next: Material) => {
    setMaterial(next);
    const i = selected();
    const wall = i === undefined ? undefined : walls()[i];
    if (wall && wall.material !== next) {
      commit(walls().map((w, j) => (j === i ? { ...w, material: next } : w)));
    }
  };

  /** Keys while drawing walls; true when one was handled (Esc that ends nothing isn't). */
  const key = (event: KeyboardEvent): boolean => {
    const target = event.target as HTMLElement | null;
    if (target?.closest("input, select, textarea")) return false;
    if ((event.key === "z" || event.key === "Z") && (event.ctrlKey || event.metaKey)) {
      undo();
      return true;
    }
    if (event.key === "Escape" || event.key === "Enter") {
      if (chain()) {
        setChain(undefined);
        setHover(undefined);
        return true;
      }
      if (selected() !== undefined) {
        setSelected(undefined);
        return true;
      }
      return false;
    }
    if ((event.key === "Delete" || event.key === "Backspace") && selected() !== undefined) {
      remove();
      return true;
    }
    return false;
  };

  return {
    walls,
    shown,
    material,
    pickMaterial,
    chain,
    hover,
    selected,
    dragging: () => drag() !== undefined,
    canUndo: () => history().length > 0,
    undo,
    remove,
    click,
    move,
    grab,
    pull,
    release,
    key,
    end,
  };
}

export type WallsState = ReturnType<typeof createWalls>;
