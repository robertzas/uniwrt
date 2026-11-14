import type { JSX } from "@solidjs/web";
import { Maximize, Minus, Plus } from "lucide";
import { createEffect, createSignal, onSettled } from "solid-js";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";

/** A point on the plan, in its image's pixels. */
export interface PlanPoint {
  x: number;
  y: number;
}

/** What overlays draw with: the zoom (screen px per plan px) and plan ↔ screen conversions. */
export interface PlanFrame {
  zoom: () => number;
  /** A plan point's place in the canvas, in screen pixels. */
  project: (point: PlanPoint) => PlanPoint;
  /** The plan point under a pointer, from its client coordinates. */
  toPlan: (clientX: number, clientY: number) => PlanPoint;
}

const MAX_ZOOM = 8;
const t = strings.floors.canvas;

/**
 * A floor's plan in a pan-and-zoom view (D-117): drag to pan, wheel or the buttons to zoom,
 * fitted on first show. `underlay` draws on the plan in its own pixels (a heatmap), `children`
 * over it in screen space (markers keep their size at any zoom); `picking` turns clicks into
 * `onPick` with the plan point under the pointer, and `onHover` follows the pointer while it
 * isn't panning (undefined once it leaves).
 */
export function PlanCanvas(props: {
  src: string;
  width: number;
  height: number;
  label: string;
  picking?: boolean;
  onPick?: (point: PlanPoint, event: PointerEvent) => void;
  onHover?: (point: PlanPoint | undefined, event: PointerEvent) => void;
  /** Called with each zoom, for scale bars. */
  onZoom?: (zoom: number) => void;
  /** Drawn on the plan, in its pixels. */
  underlay?: JSX.Element;
  children?: (frame: PlanFrame) => JSX.Element;
  /** HTML laid over the canvas's corner (a scale bar, a hint). */
  overlay?: JSX.Element;
}) {
  let svg!: SVGSVGElement;
  const [view, setView] = createSignal({ x: 0, y: 0, zoom: 1 });
  let fitZoom = 1;
  let drag: { id: number; x: number; y: number; moved: boolean } | undefined;

  const size = () => {
    const box = svg?.getBoundingClientRect();
    return { width: box?.width || props.width, height: box?.height || props.height };
  };
  const fitView = () => {
    const { width, height } = size();
    const zoom = Math.min(width / props.width, height / props.height) * 0.95 || 1;
    fitZoom = zoom;
    setView({
      x: (width - props.width * zoom) / 2,
      y: (height - props.height * zoom) / 2,
      zoom,
    });
  };
  /** Zooms by `factor` about a point of the canvas (its center by default). */
  const zoomBy = (factor: number, at?: PlanPoint) => {
    const { width, height } = size();
    const anchor = at ?? { x: width / 2, y: height / 2 };
    setView((v) => {
      const zoom = Math.min(MAX_ZOOM, Math.max(fitZoom / 2, v.zoom * factor));
      const k = zoom / v.zoom;
      return { x: anchor.x - (anchor.x - v.x) * k, y: anchor.y - (anchor.y - v.y) * k, zoom };
    });
  };
  const local = (clientX: number, clientY: number) => {
    const box = svg.getBoundingClientRect();
    return { x: clientX - box.left, y: clientY - box.top };
  };
  const frame: PlanFrame = {
    zoom: () => view().zoom,
    project: (point) => {
      const v = view();
      return { x: v.x + point.x * v.zoom, y: v.y + point.y * v.zoom };
    },
    toPlan: (clientX, clientY) => {
      const v = view();
      const at = local(clientX, clientY);
      return { x: (at.x - v.x) / v.zoom, y: (at.y - v.y) / v.zoom };
    },
  };

  createEffect(
    () => `${props.src}|${props.width}|${props.height}`,
    () => {
      fitView();
    },
  );
  createEffect(
    () => view().zoom,
    (zoom) => {
      props.onZoom?.(zoom);
    },
  );
  onSettled(() => {
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      zoomBy(Math.exp(-event.deltaY * 0.002), local(event.clientX, event.clientY));
    };
    svg.addEventListener("wheel", onWheel, { passive: false });
    const observer = new ResizeObserver(() => {
      fitView();
    });
    observer.observe(svg);
    return () => {
      svg.removeEventListener("wheel", onWheel);
      observer.disconnect();
    };
  });

  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, moved: false };
    svg.setPointerCapture?.(event.pointerId);
  };
  const onPointerMove = (event: PointerEvent) => {
    if (!drag?.moved) props.onHover?.(frame.toPlan(event.clientX, event.clientY), event);
    if (!drag || drag.id !== event.pointerId) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 4) return;
    drag = { ...drag, x: event.clientX, y: event.clientY, moved: true };
    setView((v) => ({ ...v, x: v.x + dx, y: v.y + dy }));
  };
  const onPointerUp = (event: PointerEvent) => {
    if (!drag || drag.id !== event.pointerId) return;
    const clicked = !drag.moved;
    drag = undefined;
    if (clicked && props.picking) {
      props.onPick?.(frame.toPlan(event.clientX, event.clientY), event);
    }
  };

  const button =
    "grid size-8 place-items-center rounded-control text-fg-muted hover:bg-surface-raised hover:text-fg focus-visible:outline-2 focus-visible:outline-accent";
  return (
    <div class="relative h-[min(70vh,44rem)] min-h-80 overflow-hidden rounded-control bg-canvas">
      <svg
        ref={svg}
        role="img"
        aria-label={props.label}
        class={[
          "block size-full touch-none select-none",
          props.picking ? "cursor-crosshair" : "cursor-grab active:cursor-grabbing",
        ]}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          drag = undefined;
        }}
        onPointerLeave={(event) => props.onHover?.(undefined, event)}
      >
        <g transform={`translate(${view().x} ${view().y}) scale(${view().zoom})`}>
          <rect width={props.width} height={props.height} fill="#fff" />
          <image href={props.src} width={props.width} height={props.height} />
          {props.underlay}
        </g>
        {props.children?.(frame)}
      </svg>
      <div class="absolute top-2 right-2 flex flex-col rounded-control border border-edge bg-surface/90 shadow-card backdrop-blur">
        <button type="button" class={button} aria-label={t.zoomIn} onClick={() => zoomBy(1.4)}>
          <Icon icon={Plus} class="size-4" />
        </button>
        <button type="button" class={button} aria-label={t.zoomOut} onClick={() => zoomBy(1 / 1.4)}>
          <Icon icon={Minus} class="size-4" />
        </button>
        <button type="button" class={button} aria-label={t.fit} onClick={fitView}>
          <Icon icon={Maximize} class="size-4" />
        </button>
      </div>
      {props.overlay}
    </div>
  );
}
