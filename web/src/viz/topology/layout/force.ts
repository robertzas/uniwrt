import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import type { Medium } from "../../../api/gen/Medium";
import type { VertexKind } from "../scene";
import type { Point } from "./tree";

/** What the layout needs of a vertex. */
export interface LayoutVertex {
  id: string;
  kind: VertexKind;
  radius: number;
}

export interface LayoutLink {
  source: string;
  target: string;
  medium: Medium;
}

/** Messages to the layout (the worker, or the runner on the main thread). */
export type LayoutRequest =
  | {
      type: "scene";
      /** Answers carry it, so positions are read against the scene they belong to. */
      seq: number;
      vertices: LayoutVertex[];
      links: LayoutLink[];
      pins: Record<string, Point>;
    }
  | { type: "drag"; id: string; x: number; y: number }
  /** A drag ended: the vertex stays where it was dropped when `pinned`, else it is let go. */
  | { type: "release"; id: string; pinned: boolean }
  | { type: "pins"; pins: Record<string, Point> }
  | { type: "stop" };

/** x and y of each of the scene's vertices in turn. */
export interface LayoutPositions {
  type: "positions";
  seq: number;
  positions: Float32Array;
  /** The layout came to rest: no more positions follow until something changes. */
  settled: boolean;
}

interface Body extends SimulationNodeDatum {
  id: string;
  kind: VertexKind;
  radius: number;
}

type Spring = SimulationLinkDatum<Body> & { medium: Medium };

/** World units between the ends of a link, by medium. */
const lengths: Record<Medium, number> = { wan: 110, ethernet: 120, mesh: 160, wifi: 46 };
const FRAME_MS = 16;

/** A deterministic random source (an LCG), so the same scene lays out the same way. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return state / 2 ** 32;
  };
}

/** A stable angle for a vertex, to place a new one beside its neighbour. */
function angleOf(id: string): number {
  let hash = 0;
  for (let i = 0; i < id.length; i++) hash = (Math.imul(hash, 31) + id.charCodeAt(i)) >>> 0;
  return (hash / 2 ** 32) * 2 * Math.PI;
}

const isClient = (kind: VertexKind) => kind === "client";

/**
 * d3-force over the scene: springs along the links, repulsion (strong between devices, weak
 * between clients), collision and a gentle pull to the centre. Pinned vertices stay put. It
 * ticks once per frame while it moves and posts every vertex's position; vertices keep their
 * place from one scene to the next, and new ones start beside a neighbour.
 */
export class ForceRunner {
  readonly #post: (message: LayoutPositions) => void;
  readonly #simulation: Simulation<Body, Spring>;
  #bodies: Body[] = [];
  #seq = 0;
  #pins: Record<string, Point> = {};
  #dragging: string | undefined;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(post: (message: LayoutPositions) => void) {
    this.#post = post;
    this.#simulation = forceSimulation<Body, Spring>()
      .randomSource(seeded(1))
      .force(
        "link",
        forceLink<Body, Spring>()
          .id((body) => body.id)
          .distance((spring) => lengths[spring.medium])
          .strength((spring) => (spring.medium === "wifi" ? 0.9 : 0.5)),
      )
      .force(
        "charge",
        forceManyBody<Body>()
          .strength((body) => (isClient(body.kind) ? -40 : -520))
          .distanceMax(700),
      )
      .force(
        "collide",
        forceCollide<Body>((body) => body.radius + (isClient(body.kind) ? 3 : 10)),
      )
      .force("x", forceX<Body>(0).strength(0.04))
      .force("y", forceY<Body>(0).strength(0.04))
      .stop();
  }

  handle(request: LayoutRequest) {
    switch (request.type) {
      case "scene":
        this.#setScene(request);
        break;
      case "drag": {
        const body = this.#body(request.id);
        if (!body) return;
        this.#dragging = request.id;
        body.fx = request.x;
        body.fy = request.y;
        this.#simulation.alphaTarget(0.25);
        this.#run();
        break;
      }
      case "release": {
        const body = this.#body(request.id);
        this.#dragging = undefined;
        this.#simulation.alphaTarget(0);
        if (!body) return;
        if (request.pinned && body.fx != null && body.fy != null) {
          this.#pins[body.id] = { x: body.fx, y: body.fy };
        } else {
          delete this.#pins[body.id];
          body.fx = null;
          body.fy = null;
        }
        this.#run();
        break;
      }
      case "pins":
        this.#pins = { ...request.pins };
        for (const body of this.#bodies) this.#applyPin(body);
        this.#simulation.alpha(Math.max(this.#simulation.alpha(), 0.3));
        this.#run();
        break;
      case "stop":
        clearTimeout(this.#timer);
        this.#timer = undefined;
        break;
    }
  }

  /** Runs the layout to rest at once, for tests and static renders. */
  settle(maxTicks = 600) {
    clearTimeout(this.#timer);
    this.#timer = undefined;
    for (let i = 0; i < maxTicks && this.#simulation.alpha() > this.#simulation.alphaMin(); i++) {
      this.#simulation.tick();
    }
    this.#emit(true);
  }

  #setScene(request: Extract<LayoutRequest, { type: "scene" }>) {
    this.#seq = request.seq;
    this.#pins = { ...request.pins };
    const old = new Map(this.#bodies.map((body) => [body.id, body]));
    const fresh: Body[] = [];
    this.#bodies = request.vertices.map((vertex) => {
      const body = old.get(vertex.id) ?? {
        id: vertex.id,
        kind: vertex.kind,
        radius: vertex.radius,
      };
      body.kind = vertex.kind;
      body.radius = vertex.radius;
      if (!old.has(vertex.id)) fresh.push(body);
      this.#applyPin(body);
      return body;
    });
    const byId = new Map(this.#bodies.map((body) => [body.id, body]));
    // New vertices start beside a neighbour that has a place, so the layout doesn't jump.
    for (const body of fresh) {
      if (body.x != null) continue;
      for (const link of request.links) {
        const other =
          link.source === body.id ? link.target : link.target === body.id ? link.source : undefined;
        const near = other === undefined ? undefined : byId.get(other);
        if (near?.x == null || near.y == null || fresh.includes(near)) continue;
        const angle = angleOf(body.id);
        const distance = lengths[link.medium];
        body.x = near.x + Math.cos(angle) * distance;
        body.y = near.y + Math.sin(angle) * distance;
        break;
      }
    }
    this.#simulation.nodes(this.#bodies);
    const springs: Spring[] = request.links
      .filter((link) => byId.has(link.source) && byId.has(link.target))
      .map((link) => ({ source: link.source, target: link.target, medium: link.medium }));
    this.#simulation.force<ReturnType<typeof forceLink<Body, Spring>>>("link")?.links(springs);
    const changed = fresh.length > 0 || old.size !== this.#bodies.length;
    this.#simulation.alpha(
      changed ? (old.size === 0 ? 1 : 0.5) : Math.max(this.#simulation.alpha(), 0.1),
    );
    this.#run();
  }

  #applyPin(body: Body) {
    const pin = this.#pins[body.id];
    if (pin) {
      body.fx = pin.x;
      body.fy = pin.y;
    } else if (this.#dragging !== body.id) {
      body.fx = null;
      body.fy = null;
    }
  }

  #body(id: string): Body | undefined {
    return this.#bodies.find((body) => body.id === id);
  }

  #run() {
    if (this.#timer !== undefined) return;
    const frame = () => {
      this.#timer = undefined;
      this.#simulation.tick();
      const settled =
        this.#dragging === undefined && this.#simulation.alpha() <= this.#simulation.alphaMin();
      this.#emit(settled);
      if (!settled) this.#timer = setTimeout(frame, FRAME_MS);
    };
    this.#timer = setTimeout(frame, 0);
  }

  #emit(settled: boolean) {
    const positions = new Float32Array(this.#bodies.length * 2);
    this.#bodies.forEach((body, i) => {
      positions[2 * i] = body.fx ?? body.x ?? 0;
      positions[2 * i + 1] = body.fy ?? body.y ?? 0;
    });
    this.#post({ type: "positions", seq: this.#seq, positions, settled });
  }
}
