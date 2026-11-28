import { CircleCheck, Router } from "lucide";
import { createMemo, For, Show, useContext } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { holding, pendingNodes } from "../../lib/config";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { ApplyProgress } from "./ApplyProgress";

const t = strings.changes.viewer;

/**
 * `/changes` for viewers (D-151, M9.5d): the managed devices the latest revision has yet to
 * reach, from the live `config` topic, and an apply while it runs. No plan: reading one logs in
 * to every device, and only admins apply or discard.
 */
export function ViewerChanges() {
  const live = useContext(LiveContext);
  const [jobs] = createJobsView(live);
  const running = createMemo(() =>
    Object.values(jobs.byId).find((job) => job.kind === "apply" && job.state === "running"),
  );
  const pending = createMemo(() => pendingNodes(live.config, live.fleet.nodes));
  const where = (node: NodeSummary) => {
    const held = holding(live.config, node.id, node.mode);
    return held.kind === "behind" ? t.at(held.revision) : t.never;
  };
  return (
    <Show
      when={running()}
      fallback={
        <Card aria-label={t.title}>
          <Show
            when={pending().length > 0}
            fallback={<EmptyState icon={CircleCheck} {...t.none} />}
          >
            <CardHeader title={t.title} description={t.description(live.config.head?.id ?? 0)} />
            <ul class="divide-y divide-edge border-edge border-t">
              <For each={pending()}>
                {(node) => (
                  <li class="flex items-center gap-3 px-4 py-3 text-sm" aria-label={node.name}>
                    <Icon icon={Router} class="size-4 shrink-0 text-fg-subtle" />
                    <a
                      href={`/devices/${node.id}`}
                      class="min-w-0 flex-1 truncate font-medium hover:underline"
                    >
                      {node.name}
                    </a>
                    <span class="text-fg-muted text-xs">{where(node)}</span>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Card>
      }
    >
      {(job) => <ApplyProgress job={job()} onAgain={() => {}} />}
    </Show>
  );
}
