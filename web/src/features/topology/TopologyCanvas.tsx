import { Cable, Globe, Network, RadioReceiver, RadioTower, Router, Wifi } from "lucide";
import { createEffect, createMemo, onSettled } from "solid-js";
import { createLayoutClient, type LayoutClient } from "./layout/client";
import type { LayoutPositions } from "./layout/force";
import { type Point, treeLayout } from "./layout/tree";
import { iconPathData } from "./render/icons";
import { readPalette } from "./render/palette";
import { TopologyRenderer } from "./render/renderer";
import type { Scene, VertexKind } from "./scene";

/** What the toolbar asks of the canvas. */
export interface CanvasControls {
  fit(): void;
  zoom(factor: number): void;
}

const icons: Partial<Record<VertexKind, Parameters<typeof iconPathData>[0]>> = {
  internet: Globe,
  gateway: Router,
  ap: Wifi,
  bridge: RadioReceiver,
  switch: Network,
  peer: RadioTower,
  client: Cable,
};

/** What makes two scenes lay out alike: their vertices and links, not the links' numbers. */
function shapeOf(scene: Scene): string {
  const vertices = scene.vertices.map((vertex) => `${vertex.id}/${vertex.radius}`).join(",");
  const links = scene.links.map((link) => `${link.source}>${link.target}`).join(",");
  return `${scene.view}|${vertices}|${links}`;
}

/**
 * The map: the renderer on a canvas, laid out by the tree layout or by d3-force in a worker.
 * Dragging a vertex in a force view pins it where it is dropped (`onPin`).
 */
export function TopologyCanvas(props: {
  scene: Scene;
  pins: Record<string, Point>;
  selected: string | null;
  flows: boolean;
  label: string;
  onSelect: (id: string | null) => void;
  onActivate: (id: string) => void;
  onPin: (id: string, at: Point) => void;
  controls: (controls: CanvasControls) => void;
}) {
  let canvas!: HTMLCanvasElement;
  let renderer: TopologyRenderer | undefined;
  let layout: LayoutClient | undefined;
  let seq = 0;
  let sent: { seq: number; ids: string[] } | undefined;
  // The latest of each prop, as the effects below were handed them: the renderer comes later.
  let scene: Scene | undefined;
  let shaped: Scene | undefined;
  let pins: Record<string, Point> = {};
  let selected: string | null = null;
  let flows = false;

  /** The scene, but only anew when its shape changes: that is all the layout needs of it. */
  const layoutScene = createMemo(() => ({ shape: shapeOf(props.scene), scene: props.scene }), {
    equals: (a: { shape: string }, b: { shape: string }) => a.shape === b.shape,
  });

  /** Lays the scene out: the tree at once, a force view in the worker. */
  const layOut = (scene: Scene) => {
    seq += 1;
    renderer?.setDraggable(scene.view !== "tree");
    if (scene.view === "tree") {
      sent = undefined;
      layout?.send({ type: "stop" });
      renderer?.setTargets(treeLayout(scene));
      return;
    }
    sent = { seq, ids: scene.vertices.map((vertex) => vertex.id) };
    layout?.send({
      type: "scene",
      seq,
      vertices: scene.vertices.map(({ id, kind, radius }) => ({ id, kind, radius })),
      links: scene.links.map(({ source, target, medium }) => ({ source, target, medium })),
      pins: { ...pins },
    });
  };

  const onPositions = (message: LayoutPositions) => {
    if (!renderer || message.seq !== sent?.seq) return;
    const targets = new Map<string, Point>();
    sent.ids.forEach((id, i) => {
      targets.set(id, { x: message.positions[2 * i] ?? 0, y: message.positions[2 * i + 1] ?? 0 });
    });
    renderer.setTargets(targets);
  };

  onSettled(() => {
    const paths = new Map<VertexKind, Path2D>();
    if (typeof Path2D !== "undefined") {
      for (const [kind, icon] of Object.entries(icons)) {
        paths.set(kind as VertexKind, new Path2D(iconPathData(icon)));
      }
    }
    const view = new TopologyRenderer(
      canvas,
      {
        select: (id) => props.onSelect(id),
        activate: (id) => props.onActivate(id),
        drag: (id, at, done) => {
          if (!done) {
            layout?.send({ type: "drag", id, x: at.x, y: at.y });
            return;
          }
          layout?.send({ type: "release", id, pinned: true });
          props.onPin(id, at);
        },
      },
      readPalette(canvas),
      paths,
    );
    renderer = view;
    layout = createLayoutClient(onPositions);
    props.controls({ fit: () => view.fit(), zoom: (factor) => view.zoom(factor) });

    const repaint = () => view.setPalette(readPalette(canvas));
    const themes = new MutationObserver(repaint);
    themes.observe(document.documentElement, { attributeFilter: ["data-theme", "data-skin"] });
    const dark = matchMedia("(prefers-color-scheme: dark)");
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    const onMotion = () => view.setReducedMotion(motion.matches);
    dark.addEventListener("change", repaint);
    motion.addEventListener("change", onMotion);
    onMotion();
    // The effects below may have run before the renderer existed: hand it what they had.
    view.setPinned(new Set(Object.keys(pins)));
    view.setSelected(selected);
    view.setFlows(flows);
    if (scene) view.setScene(scene);
    if (shaped) layOut(shaped);
    view.fit();
    return () => {
      themes.disconnect();
      dark.removeEventListener("change", repaint);
      motion.removeEventListener("change", onMotion);
      layout?.dispose();
      view.dispose();
      renderer = undefined;
    };
  });

  // Every change of the data reaches the renderer; only a change of shape re-runs the layout.
  createEffect(
    () => props.scene,
    (value) => {
      scene = value;
      renderer?.setScene(value);
    },
  );
  createEffect(
    () => layoutScene().scene,
    (value) => {
      shaped = value;
      if (renderer) layOut(value);
    },
  );
  createEffect(
    () => props.scene.view,
    () => renderer?.fit(),
    { defer: true },
  );
  createEffect(
    () => props.pins,
    (value) => {
      pins = value;
      renderer?.setPinned(new Set(Object.keys(value)));
      layout?.send({ type: "pins", pins: { ...value } });
    },
  );
  createEffect(
    () => props.selected,
    (value) => {
      selected = value;
      renderer?.setSelected(value);
    },
  );
  createEffect(
    () => props.flows,
    (value) => {
      flows = value;
      renderer?.setFlows(value);
    },
  );

  return (
    <canvas
      ref={canvas}
      tabindex="0"
      role="img"
      aria-label={props.label}
      class="block size-full touch-none select-none outline-none focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
    />
  );
}
