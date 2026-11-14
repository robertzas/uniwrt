import { createEffect, createSignal } from "solid-js";
import type { Floor } from "../../../api/gen/Floor";
import type { Material } from "../../../api/gen/Material";
import type { Wall } from "../../../api/gen/Wall";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { showToast } from "../../../ui/Toast";
import type { PlanPoint } from "../../../viz/floorplan/PlanCanvas";
import { metersPerPx } from "../coverage/model";
import { type End, endsAt, hitWall, moveEnds, nearestEnd, type Snapped, snap } from "./snap";
import { tracePlan } from "./trace/read";
import type { TracedWall } from "./trace/trace";

const t = strings.floors.walls;

/**
 * Plain copies of `walls`: the query's data is reconciled in place when an answer arrives
 * (D-047), so a list kept for undo must not share its arrays or objects.
 */
const copy = (walls: readonly Wall[]): Wall[] =>
  walls.map((w) => ({ from: { ...w.from }, to: { ...w.to }, material: w.material }));
const same = (a: readonly Wall[], b: readonly Wall[]) => JSON.stringify(a) === JSON.stringify(b);
/** The controller keeps at most this many walls per floor. */
export const MAX_WALLS = 1000;
/** What undo goes back to: the walls and the traced suggestions still open. */
interface Step {
  walls: Wall[];
  suggestions: readonly TracedWall[];
}
/** Undo keeps this many steps per floor shown. */
const UNDO_STEPS = 50;
/** A click this soon after a wall ended on the same spot is the double-click's second half. */
const DOUBLE_CLICK_MS = 500;

/**
 * The walls of the floor shown and what drawing them is doing (M6.2c): the polyline being drawn,
 * the wall selected, an end being dragged, and the walls traced from the plan as suggestions
 * (M6.2c2: traced when Walls opens on a floor without walls, or on request; kept in memory until
 * accepted). Every edit shows at once (the heatmap follows the drawn walls) and saves the whole
 * list in the background, one request at a time, the latest list winning; a failed save goes
 * back to the controller's walls.
 */
export function createWalls(props: {
  floor: () => Floor | undefined;
  onSaved: (floor: Floor) => void;
}) {
  const [draft, setDraft] = createSignal<readonly Wall[]>();
  const [history, setHistory] = createSignal<readonly Step[]>([]);
  const [material, setMaterial] = createSignal<Material>("drywall");
  /** The corner drawn last, while drawing. */
  const [chain, setChain] = createSignal<PlanPoint>();
  const [selected, setSelected] = createSignal<number>();
  /** Where the pointer is, snapped, while drawing. */
  const [hover, setHover] = createSignal<Snapped>();
  /** The ends being dragged and where to. */
  const [drag, setDrag] = createSignal<{ ends: End[]; to: PlanPoint }>();
  /** The traced walls not yet accepted or deleted; `offered` mirrors them for reads after writes. */
  const [suggestions, setSuggestions] = createSignal<readonly TracedWall[]>([]);
  let offered: readonly TracedWall[] = [];
  const offer = (next: readonly TracedWall[]) => {
    offered = next;
    setSuggestions(next);
  };
  /** The suggestion selected. */
  const [picked, setPicked] = createSignal<number>();
  const [tracing, setTracing] = createSignal(false);
  /** A trace asked for found nothing (shown until the next click). */
  const [nothing, setNothing] = createSignal(false);

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
  /** Keeps the state now for undo. */
  const record = () => {
    // Read before the write: an updater may run when Solid flushes, after `latest` moved on.
    const before: Step = { walls: copy(current()), suggestions: offered };
    setHistory((h) => [...h.slice(-(UNDO_STEPS - 1)), before]);
  };
  /** Saves `next` as the walls, with `suggestions` left open when given. */
  const commit = (next: readonly Wall[], suggestions?: readonly TracedWall[]) => {
    record();
    if (suggestions) offer(suggestions);
    latest = copy(next);
    setDraft(latest);
    void flush();
  };
  /** Leaves `next` open as the suggestions; the walls don't change. */
  const reoffer = (next: readonly TracedWall[]) => {
    record();
    offer(next);
  };
  const undo = () => {
    const h = history();
    const last = h[h.length - 1];
    if (!last) return;
    setHistory(h.slice(0, -1));
    setSelected(undefined);
    setPicked(undefined);
    setChain(undefined);
    offer(last.suggestions);
    if (same(last.walls, current())) return;
    latest = last.walls;
    setDraft(last.walls);
    void flush();
  };

  /** Plans (by floor and version) traced already, so opening Walls again doesn't. */
  const traced = new Set<string>();
  /** Bumped by every trace and floor change: a trace finishing after either is dropped. */
  let token = 0;
  const suggest = async (auto: boolean) => {
    const floor = props.floor();
    const image = floor?.image;
    if (!floor || !image || tracing()) return;
    traced.add(`${floor.id}:${image.version}`);
    const mine = ++token;
    setTracing(true);
    setNothing(false);
    try {
      const existing = current();
      const found = await tracePlan(api.floors.imageUrl(floor.id, image.version), {
        width: image.width_px,
        height: image.height_px,
        metersPerPx: metersPerPx(floor),
        existing,
        limit: Math.max(0, MAX_WALLS - existing.length),
      });
      if (mine !== token) return;
      if (!found) {
        if (!auto) showToast({ title: t.unreadable, tone: "crit" });
        return;
      }
      if (found.length === 0 && !auto) setNothing(true);
      setPicked(undefined);
      if (auto) offer(found);
      else reoffer(found);
    } finally {
      if (mine === token) setTracing(false);
    }
  };
  /** Walls opened: a floor without walls whose plan wasn't traced yet is traced. */
  const open = () => {
    const floor = props.floor();
    const image = floor?.image;
    if (!floor || !image || current().length > 0 || offered.length > 0) return;
    if (!traced.has(`${floor.id}:${image.version}`)) void suggest(true);
  };

  // Another floor starts afresh.
  createEffect(
    () => props.floor()?.id,
    () => {
      latest = undefined;
      setDraft(undefined);
      setHistory([]);
      token++;
      setTracing(false);
      offer([]);
      end();
    },
  );

  function end() {
    setChain(undefined);
    setSelected(undefined);
    setPicked(undefined);
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
    setNothing(false);
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
      setPicked(undefined);
      setChain(corner);
      return;
    }
    const hit = hitWall(point, list, tolerance);
    if (hit !== undefined && hit !== selected()) {
      setSelected(hit);
      setPicked(undefined);
      setMaterial((list[hit] as Wall).material);
      return;
    }
    const proposal = hit === undefined ? hitWall(point, offered, tolerance) : undefined;
    if (proposal !== undefined) {
      setSelected(undefined);
      setPicked(proposal);
      setMaterial((offered[proposal] as TracedWall).material);
      return;
    }
    setSelected(undefined);
    setPicked(undefined);
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
  /** The selected suggestion (or all of them) made walls, as traced. */
  const accept = () => {
    const i = picked();
    const s = i === undefined ? undefined : offered[i];
    if (!s) return;
    setPicked(undefined);
    commit(
      [...current(), ...copy([s])],
      offered.filter((_, j) => j !== i),
    );
  };
  const acceptAll = () => {
    if (offered.length === 0) return;
    setPicked(undefined);
    commit([...current(), ...copy(offered)], []);
  };
  const dismiss = () => {
    const i = picked();
    if (i === undefined) return;
    setPicked(undefined);
    reoffer(offered.filter((_, j) => j !== i));
  };
  const dismissAll = () => {
    if (offered.length === 0) return;
    setPicked(undefined);
    reoffer([]);
  };

  const pickMaterial = (next: Material) => {
    setMaterial(next);
    const p = picked();
    if (p !== undefined && offered[p] && offered[p].material !== next) {
      reoffer(offered.map((s, j) => (j === p ? { ...s, material: next } : s)));
      return;
    }
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
      if (picked() !== undefined) {
        if (event.key === "Enter") accept();
        else setPicked(undefined);
        return true;
      }
      if (selected() !== undefined) {
        setSelected(undefined);
        return true;
      }
      return false;
    }
    if (event.key === "Delete" || event.key === "Backspace") {
      if (picked() !== undefined) {
        dismiss();
        return true;
      }
      if (selected() !== undefined) {
        remove();
        return true;
      }
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
    suggestions,
    picked,
    tracing,
    nothing,
    suggest: () => void suggest(false),
    open,
    accept,
    acceptAll,
    dismiss,
    dismissAll,
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
