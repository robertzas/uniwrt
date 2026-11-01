import { useNavigate } from "@solidjs/router";
import { ArrowUpRight } from "lucide";
import { createMemo, Show } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import type { Graph } from "../../viz/topology/graph";
import { buildScene, sceneCounts } from "../../viz/topology/scene";
import { TopologyCanvas } from "../../viz/topology/TopologyCanvas";

const t = strings.dashboard.map;

/**
 * The topology mini-map: the tree view with each host's clients grouped, flows on, on a fixed
 * camera (the page keeps the wheel and touch). A device opens its page, a client its own; anything
 * else opens the full map.
 */
export function MiniMap(props: { graph: Graph | undefined; nodes: readonly NodeSummary[] }) {
  const navigate = useNavigate();
  const names = createMemo(() => new Map(props.nodes.map((node) => [node.id, node.name])));
  const scene = createMemo(() => {
    const graph = props.graph;
    if (!graph || graph.nodes.size === 0) return undefined;
    return buildScene(graph, "tree", {
      clustered: () => true,
      nodeName: (node) => (node.node === null ? undefined : names().get(node.node)),
    });
  });
  const counts = createMemo(() =>
    props.graph ? sceneCounts(props.graph) : { devices: 0, clients: 0 },
  );
  const open = (id: string | null) => {
    if (id === null) return;
    const vertex = props.graph?.nodes.get(id);
    if (vertex?.node != null) navigate(`/devices/${vertex.node}`);
    else if (vertex?.kind === "client" && vertex.mac) navigate(`/clients/${vertex.mac}`);
    else navigate("/topology");
  };

  return (
    <Card class="flex min-w-0 flex-col" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.counts(counts().devices, counts().clients)}
        actions={
          <a
            href="/topology"
            class="inline-flex items-center gap-1 font-medium text-accent hover:underline"
          >
            {t.open}
            <Icon icon={ArrowUpRight} class="size-3.5" />
          </a>
        }
      />
      <div class="h-72 min-h-0 flex-1 px-1 pb-1 lg:h-auto lg:min-h-72">
        <Show
          when={scene()}
          fallback={
            <p class="grid h-full place-items-center px-4 text-center text-fg-subtle text-sm">
              {t.empty}
            </p>
          }
        >
          {(current) => (
            <TopologyCanvas
              scene={current()}
              flows={true}
              fixed={true}
              label={strings.topology.canvas(counts().devices, counts().clients)}
              onSelect={open}
            />
          )}
        </Show>
      </div>
    </Card>
  );
}
