import { useSearchParams } from "@solidjs/router";
import { History, Info, Maximize, Network, PinOff, ZoomIn, ZoomOut } from "lucide";
import { createEffect, createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import { api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { EmptyState } from "../../ui/EmptyState";
import { showToast } from "../../ui/Toast";
import { DetailsPanel } from "./DetailsPanel";
import { createLiveGraph } from "./graph";
import { Legend } from "./Legend";
import type { Point } from "./layout/tree";
import { ReplayBar } from "./ReplayBar";
import { createReplay } from "./replay";
import { buildScene, clusterId, sceneCounts, type ViewKind, vertexLabel, viewKinds } from "./scene";
import { type CanvasControls, TopologyCanvas } from "./TopologyCanvas";
import { ViewSwitch } from "./ViewSwitch";

const t = strings.topology;

type Toggles = Record<ViewKind, ReadonlySet<string>>;

/**
 * `/topology`: the live map of the network (features/observability.md, Topology), in the view
 * that `?view=` names, with details of what is selected and a replay of the last 24 h.
 */
export function TopologyPage() {
  const live = useContext(LiveContext);
  const [params, setParams] = useSearchParams<{ view?: string }>();
  const view = (): ViewKind => viewKinds.find((kind) => kind === params.view) ?? "tree";
  const liveGraph = createLiveGraph(live);
  const replay = createReplay();
  const [selected, setSelected] = createSignal<string | null>(null);
  /** Hosts whose clients differ from the view's default: grouped in the tree, apart elsewhere. */
  const [toggles, setToggles] = createSignal<Toggles>({
    tree: new Set(),
    rf: new Set(),
    clients: new Set(),
  });
  const [pins, setPins] = createSignal<Record<string, Point>>({});
  const [legend, setLegend] = createSignal(false);
  let controls: CanvasControls | undefined;

  onSettled(() => {
    api.topology.layout().then(
      (list) =>
        setPins(Object.fromEntries(list.map((pin) => [pin.vertex, { x: pin.x, y: pin.y }]))),
      () => {},
    );
  });
  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  const replaying = () => replay.at() !== null;
  const graph = () => (replaying() ? (replay.graph() ?? liveGraph()) : liveGraph());
  const names = createMemo(() => new Map(live.fleet.nodes.map((node) => [node.id, node.name])));
  const grouped = (host: string) => (view() === "tree") !== toggles()[view()].has(host);
  const scene = createMemo(() => {
    const current = graph();
    if (!current) return undefined;
    return buildScene(current, view(), {
      clustered: grouped,
      nodeName: (node) => (node.node === null ? undefined : names().get(node.node)),
    });
  });
  const counts = createMemo(() => {
    const current = graph();
    return current ? sceneCounts(current) : { devices: 0, clients: 0 };
  });
  const labelOf = (id: string) => {
    const known = scene()?.byId.get(id)?.label;
    if (known) return known;
    const node = graph()?.nodes.get(id);
    return node
      ? vertexLabel(node, (n) => (n.node === null ? undefined : names().get(n.node)))
      : id;
  };
  /** Whether a vertex has clients to group: a host of some, or their cluster. */
  const hostOf = (id: string): string | undefined => {
    const current = scene();
    const vertex = current?.byId.get(id);
    if (!current || !vertex || current.view === "rf") return undefined;
    if (vertex.kind === "cluster") return vertex.host ?? undefined;
    if (current.byId.has(clusterId(id))) return id;
    return current.vertices.some((v) => v.host === id && v.kind === "client") ? id : undefined;
  };
  const toggleGroup = (host: string) => {
    setToggles((all) => {
      const set = new Set(all[view()]);
      if (!set.delete(host)) set.add(host);
      return { ...all, [view()]: set };
    });
  };

  const pin = (id: string, at: Point) => {
    const before = pins();
    setPins({ ...before, [id]: at });
    api.topology.pin(id, at).catch(() => {
      setPins(before);
      showToast({ title: t.pinFailed, tone: "crit" });
    });
  };
  const unpin = (id: string) => {
    const before = pins();
    const { [id]: _, ...rest } = before;
    setPins(rest);
    api.topology.unpin(id).catch(() => setPins(before));
  };
  const unpinAll = () => {
    const before = pins();
    setPins({});
    api.topology.unpinAll().catch(() => setPins(before));
  };

  return (
    <div class="flex h-[calc(100dvh-6.5rem)] min-h-[28rem] flex-col gap-3">
      <header class="flex flex-wrap items-center gap-x-3 gap-y-2">
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <Show when={replaying()}>
          <Badge tone="info" icon={History}>
            {t.replay.replaying}
          </Badge>
        </Show>
        <div class="ms-auto flex flex-wrap items-center gap-2">
          <ViewSwitch
            value={view()}
            onChange={(next) => setParams({ view: next }, { replace: true })}
          />
          <div class="flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              icon={ZoomOut}
              aria-label={t.toolbar.zoomOut}
              title={t.toolbar.zoomOut}
              onClick={() => controls?.zoom(0.8)}
            />
            <Button
              variant="ghost"
              size="sm"
              icon={ZoomIn}
              aria-label={t.toolbar.zoomIn}
              title={t.toolbar.zoomIn}
              onClick={() => controls?.zoom(1.25)}
            />
            <Button
              variant="ghost"
              size="sm"
              icon={Maximize}
              aria-label={t.toolbar.fit}
              title={t.toolbar.fit}
              onClick={() => controls?.fit()}
            />
            <Show when={view() !== "tree" && Object.keys(pins()).length > 0}>
              <Button variant="ghost" size="sm" icon={PinOff} onClick={unpinAll}>
                {t.toolbar.unpinAll}
              </Button>
            </Show>
            <Button
              variant="ghost"
              size="sm"
              icon={Info}
              aria-label={t.toolbar.legend}
              title={t.toolbar.legend}
              aria-pressed={legend() ? "true" : "false"}
              onClick={() => setLegend((open) => !open)}
            />
          </div>
        </div>
      </header>
      <div class="relative min-h-0 flex-1 overflow-hidden rounded-card border border-card-edge bg-canvas shadow-card">
        <Show
          when={scene()}
          fallback={
            <p role="status" class="py-20 text-center text-fg-subtle text-sm">
              {t.loading}…
            </p>
          }
        >
          {(current) => (
            <Show
              when={counts().devices > 0}
              fallback={
                <div class="py-12">
                  <EmptyState
                    icon={Network}
                    title={t.empty.title}
                    description={t.empty.description}
                  />
                </div>
              }
            >
              <ul class="sr-only" aria-label={t.list}>
                <For each={current().vertices}>
                  {(vertex) => (
                    <li>
                      <button
                        type="button"
                        onFocus={() => setSelected(vertex.id)}
                        onClick={() => setSelected(vertex.id)}
                      >
                        {vertex.label}, {t.kind[vertex.kind]}
                      </button>
                    </li>
                  )}
                </For>
              </ul>
              <TopologyCanvas
                scene={current()}
                pins={pins()}
                selected={selected()}
                flows={!replaying()}
                label={t.canvas(counts().devices, counts().clients)}
                onSelect={setSelected}
                onActivate={(id) => {
                  const host = hostOf(id);
                  if (host) toggleGroup(host);
                }}
                onPin={pin}
                controls={(c) => {
                  controls = c;
                }}
              />
              <Show when={selected()}>
                {(id) => (
                  <DetailsPanel
                    scene={current()}
                    selected={id()}
                    pinned={view() !== "tree" && id() in pins()}
                    grouped={hostOf(id()) === undefined ? undefined : grouped(hostOf(id()) ?? "")}
                    labelOf={labelOf}
                    onClose={() => setSelected(null)}
                    onSelect={(member) => {
                      const host = hostOf(id());
                      if (host && grouped(host)) toggleGroup(host);
                      setSelected(member);
                    }}
                    onUnpin={() => unpin(id())}
                    onToggleGroup={() => {
                      const host = hostOf(id());
                      if (!host) return;
                      toggleGroup(host);
                      if (id() === clusterId(host)) setSelected(host);
                    }}
                  />
                )}
              </Show>
              <Show when={legend()}>
                <Legend />
              </Show>
              <p class="pointer-events-none absolute top-3 left-3 hidden text-fg-subtle text-xs md:block">
                {t.toolbar.hint}
              </p>
              <div class="absolute inset-x-3 bottom-3">
                <ReplayBar replay={replay} />
              </div>
            </Show>
          )}
        </Show>
      </div>
    </div>
  );
}
