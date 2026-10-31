import type { Point } from "../layout/tree";
import type { Scene, VertexKind } from "../scene";
import { drawFrame } from "./draw";
import { type Camera, fitCamera, flowOf, hitTest, toWorld, zoomAt } from "./geometry";
import type { Palette } from "./palette";

export interface RendererEvents {
  /** A click on a vertex or link (its id), or on nothing (`null`). */
  select(id: string | null): void;
  /** A double-click on a vertex. */
  activate(id: string): void;
  /** A vertex dragged to a point of the world; `done` once dropped. */
  drag(id: string, at: Point, done: boolean): void;
}

/** How far a pointer moves before a press becomes a drag, in screen pixels. */
const DRAG_THRESHOLD = 4;
/** The share of the way to its target a vertex moves each frame (about 200 ms to settle). */
const EASE = 0.2;
const KEY_PAN = 48;

type Gesture =
  | { kind: "vertex"; id: string; start: Point; moved: boolean; hit: string | null }
  | { kind: "pan"; start: Point; camera: Camera; moved: boolean; hit: string | null }
  | { kind: "pinch"; distance: number; center: Point; camera: Camera };

/**
 * Paints a scene on a canvas (ui.md, Signature visualizations): it eases vertices to the
 * positions it is given, keeps the camera fitted until the user pans or zooms, animates flows,
 * and turns pointer, wheel and key input into camera moves and the events above. It draws only
 * while something moves, so a still map costs nothing.
 */
export class TopologyRenderer {
  readonly #canvas: HTMLCanvasElement;
  readonly #ctx: CanvasRenderingContext2D | null;
  readonly #events: RendererEvents;
  readonly #icons: ReadonlyMap<VertexKind, Path2D>;
  readonly #resize: ResizeObserver;
  readonly #pointers = new Map<number, Point>();
  readonly #listeners: [string, EventListener, AddEventListenerOptions?][] = [];
  #palette: Palette;
  #scene: Scene | undefined;
  #targets = new Map<string, Point>();
  #display = new Map<string, Point>();
  #camera: Camera = { x: 0, y: 0, k: 1 };
  #autoFit = true;
  #size = { width: 0, height: 0 };
  #selected: string | null = null;
  #hovered: string | null = null;
  #pinned: ReadonlySet<string> = new Set();
  #flows = false;
  #draggable = false;
  #reducedMotion = false;
  #gesture: Gesture | undefined;
  #frame: number | undefined;
  #disposed = false;
  readonly #clock = performance.now();

  constructor(
    canvas: HTMLCanvasElement,
    events: RendererEvents,
    palette: Palette,
    icons: ReadonlyMap<VertexKind, Path2D>,
  ) {
    this.#canvas = canvas;
    this.#ctx = canvas.getContext("2d");
    this.#events = events;
    this.#palette = palette;
    this.#icons = icons;
    this.#resize = new ResizeObserver(() => this.#measure());
    this.#resize.observe(canvas);
    this.#measure();
    this.#listen("pointerdown", (event) => this.#down(event as PointerEvent));
    this.#listen("pointermove", (event) => this.#move(event as PointerEvent));
    this.#listen("pointerup", (event) => this.#up(event as PointerEvent));
    this.#listen("pointercancel", (event) => this.#cancel(event as PointerEvent));
    this.#listen("pointerleave", () => this.#hover(null));
    this.#listen("dblclick", (event) => this.#doubleClick(event as MouseEvent));
    this.#listen("wheel", (event) => this.#wheel(event as WheelEvent), { passive: false });
    this.#listen("keydown", (event) => this.#key(event as KeyboardEvent));
  }

  get camera(): Camera {
    return this.#camera;
  }

  setScene(scene: Scene) {
    this.#scene = scene;
    for (const id of [...this.#display.keys()]) if (!scene.byId.has(id)) this.#display.delete(id);
    this.#request();
  }

  /** Where each vertex should be; new ones grow out of a linked neighbour. */
  setTargets(targets: Map<string, Point>) {
    this.#targets = targets;
    const scene = this.#scene;
    if (scene) {
      for (const vertex of scene.vertices) {
        if (this.#display.has(vertex.id)) continue;
        const target = targets.get(vertex.id);
        if (!target) continue;
        const link = scene.links.find((l) => l.target === vertex.id || l.source === vertex.id);
        const near =
          link && this.#display.get(link.source === vertex.id ? link.target : link.source);
        this.#display.set(vertex.id, near && !this.#reducedMotion ? { ...near } : { ...target });
      }
    }
    this.#request();
  }

  setPalette(palette: Palette) {
    this.#palette = palette;
    this.#request();
  }

  setSelected(id: string | null) {
    this.#selected = id;
    this.#request();
  }

  setPinned(pinned: ReadonlySet<string>) {
    this.#pinned = pinned;
    this.#request();
  }

  setFlows(flows: boolean) {
    this.#flows = flows;
    this.#request();
  }

  /** Vertices can be dragged (force layouts), or presses on them pan (the tree). */
  setDraggable(draggable: boolean) {
    this.#draggable = draggable;
  }

  setReducedMotion(reduced: boolean) {
    this.#reducedMotion = reduced;
    this.#request();
  }

  /** Fits the view to the scene, and keeps it fitted while the layout moves. */
  fit() {
    this.#autoFit = true;
    this.#camera = fitCamera(this.#targets.values(), this.#size.width, this.#size.height);
    this.#request();
  }

  zoom(factor: number) {
    this.#autoFit = false;
    this.#camera = zoomAt(this.#camera, factor, {
      x: this.#size.width / 2,
      y: this.#size.height / 2,
    });
    this.#request();
  }

  dispose() {
    this.#disposed = true;
    this.#resize.disconnect();
    for (const [type, listener, options] of this.#listeners) {
      this.#canvas.removeEventListener(type, listener, options);
    }
    if (this.#frame !== undefined) cancelAnimationFrame(this.#frame);
  }

  #listen(type: string, listener: EventListener, options?: AddEventListenerOptions) {
    this.#canvas.addEventListener(type, listener, options);
    this.#listeners.push([type, listener, options]);
  }

  #measure() {
    const rect = this.#canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    this.#size = { width: rect.width, height: rect.height };
    this.#canvas.width = Math.max(1, Math.round(rect.width * dpr));
    this.#canvas.height = Math.max(1, Math.round(rect.height * dpr));
    if (this.#autoFit) {
      this.#camera = fitCamera(this.#targets.values(), rect.width, rect.height);
    }
    this.#request();
  }

  #request() {
    if (this.#frame !== undefined || this.#disposed || !this.#ctx) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = undefined;
      if (this.#draw()) this.#request();
    });
  }

  /** Draws a frame; true while there is more to animate. */
  #draw(): boolean {
    const ctx = this.#ctx;
    const scene = this.#scene;
    if (!ctx || !scene) return false;
    let moving = false;
    const ease = this.#reducedMotion ? 1 : EASE;
    for (const [id, target] of this.#targets) {
      const shown = this.#display.get(id);
      if (!shown) continue;
      const dx = target.x - shown.x;
      const dy = target.y - shown.y;
      if (Math.abs(dx) < 0.05 && Math.abs(dy) < 0.05) {
        shown.x = target.x;
        shown.y = target.y;
        continue;
      }
      shown.x += dx * ease;
      shown.y += dy * ease;
      moving = true;
    }
    if (this.#autoFit && this.#gesture === undefined) {
      const fit = fitCamera(this.#targets.values(), this.#size.width, this.#size.height);
      const c = this.#camera;
      const step = (from: number, to: number) => from + (to - from) * ease;
      const next = { x: step(c.x, fit.x), y: step(c.y, fit.y), k: step(c.k, fit.k) };
      if (
        Math.abs(next.x - c.x) + Math.abs(next.y - c.y) > 0.1 ||
        Math.abs(next.k - c.k) > 0.0005
      ) {
        moving = true;
      }
      this.#camera = next;
    }
    const dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const flows =
      this.#flows &&
      !this.#reducedMotion &&
      scene.links.some(
        (link) => flowOf(link.edge?.traffic?.down_bps) || flowOf(link.edge?.traffic?.up_bps),
      );
    drawFrame(ctx, {
      width: this.#size.width,
      height: this.#size.height,
      camera: this.#camera,
      scene,
      positions: this.#display,
      palette: this.#palette,
      icons: this.#icons,
      selected: this.#selected,
      hovered: this.#hovered,
      pinned: this.#pinned,
      seconds: (performance.now() - this.#clock) / 1000,
      flows,
    });
    return moving || flows;
  }

  #point(event: MouseEvent): Point {
    const rect = this.#canvas.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  #hit(at: Point) {
    return this.#scene && hitTest(this.#scene, this.#display, this.#camera, at);
  }

  #hover(id: string | null) {
    if (this.#hovered === id) return;
    this.#hovered = id;
    this.#request();
  }

  #down(event: PointerEvent) {
    if (event.button !== 0) return;
    const at = this.#point(event);
    this.#canvas.setPointerCapture?.(event.pointerId);
    this.#pointers.set(event.pointerId, at);
    if (this.#pointers.size === 2) {
      const [a, b] = [...this.#pointers.values()] as [Point, Point];
      this.#gesture = {
        kind: "pinch",
        distance: Math.hypot(b.x - a.x, b.y - a.y) || 1,
        center: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 },
        camera: this.#camera,
      };
      return;
    }
    const hit = this.#hit(at);
    const id = hit?.id ?? null;
    this.#gesture =
      hit?.kind === "vertex" && this.#draggable
        ? { kind: "vertex", id: hit.id, start: at, moved: false, hit: id }
        : { kind: "pan", start: at, camera: this.#camera, moved: false, hit: id };
  }

  #move(event: PointerEvent) {
    const at = this.#point(event);
    if (this.#pointers.has(event.pointerId)) this.#pointers.set(event.pointerId, at);
    const gesture = this.#gesture;
    if (!gesture) {
      const hit = this.#hit(at);
      this.#hover(hit?.id ?? null);
      // A device that drags shows it can move; anything else that a click selects, a pointer.
      this.#canvas.style.cursor =
        hit?.kind === "vertex" && this.#draggable ? "move" : hit ? "pointer" : "grab";
      return;
    }
    if (gesture.kind === "pinch") {
      const [a, b] = [...this.#pointers.values()] as [Point, Point];
      if (!a || !b) return;
      const center = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      const zoomed = zoomAt(
        gesture.camera,
        Math.hypot(b.x - a.x, b.y - a.y) / gesture.distance,
        gesture.center,
      );
      this.#autoFit = false;
      this.#camera = {
        ...zoomed,
        x: zoomed.x + center.x - gesture.center.x,
        y: zoomed.y + center.y - gesture.center.y,
      };
      this.#request();
      return;
    }
    if (
      !gesture.moved &&
      Math.hypot(at.x - gesture.start.x, at.y - gesture.start.y) < DRAG_THRESHOLD
    ) {
      return;
    }
    gesture.moved = true;
    this.#canvas.style.cursor = "grabbing";
    if (gesture.kind === "vertex") {
      const world = toWorld(this.#camera, at);
      this.#autoFit = false;
      this.#targets.set(gesture.id, world);
      this.#display.set(gesture.id, { ...world });
      this.#events.drag(gesture.id, world, false);
    } else {
      this.#autoFit = false;
      this.#camera = {
        ...gesture.camera,
        x: gesture.camera.x + at.x - gesture.start.x,
        y: gesture.camera.y + at.y - gesture.start.y,
      };
    }
    this.#request();
  }

  #up(event: PointerEvent) {
    this.#pointers.delete(event.pointerId);
    const gesture = this.#gesture;
    if (this.#pointers.size > 0 || !gesture) return;
    this.#gesture = undefined;
    this.#canvas.style.cursor = "";
    if (gesture.kind === "pinch") return;
    if (!gesture.moved) this.#events.select(gesture.hit);
    else if (gesture.kind === "vertex") {
      this.#events.drag(gesture.id, toWorld(this.#camera, this.#point(event)), true);
    }
  }

  #cancel(event: PointerEvent) {
    this.#pointers.delete(event.pointerId);
    const gesture = this.#gesture;
    this.#gesture = undefined;
    if (gesture?.kind === "vertex" && gesture.moved) {
      const at = this.#display.get(gesture.id);
      if (at) this.#events.drag(gesture.id, at, true);
    }
  }

  #doubleClick(event: MouseEvent) {
    const hit = this.#hit(this.#point(event));
    if (hit?.kind === "vertex") this.#events.activate(hit.id);
    else if (!hit) this.fit();
  }

  #wheel(event: WheelEvent) {
    event.preventDefault();
    // A pinch on a trackpad arrives as a wheel with ctrlKey, in bigger steps.
    const factor = Math.exp(-event.deltaY * (event.ctrlKey ? 0.01 : 0.0015));
    this.#autoFit = false;
    this.#camera = zoomAt(this.#camera, factor, this.#point(event));
    this.#request();
  }

  #key(event: KeyboardEvent) {
    const pan = (dx: number, dy: number) => {
      this.#autoFit = false;
      this.#camera = { ...this.#camera, x: this.#camera.x + dx, y: this.#camera.y + dy };
      this.#request();
    };
    const actions: Record<string, () => void> = {
      "+": () => this.zoom(1.25),
      "=": () => this.zoom(1.25),
      "-": () => this.zoom(0.8),
      "0": () => this.fit(),
      ArrowLeft: () => pan(KEY_PAN, 0),
      ArrowRight: () => pan(-KEY_PAN, 0),
      ArrowUp: () => pan(0, KEY_PAN),
      ArrowDown: () => pan(0, -KEY_PAN),
      Escape: () => this.#events.select(null),
    };
    const action = actions[event.key];
    if (!action || event.metaKey || event.ctrlKey || event.altKey) return;
    event.preventDefault();
    action();
  }
}
