import { useQuery } from "@tanstack/solid-query";
import { ArrowRight, Circle, CircleCheck, X } from "lucide";
import { createMemo, For, Show, useContext } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { api } from "../../lib/api";
import { dismissed, setDismissed } from "../../lib/checklist";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import type { Graph } from "../../viz/topology/graph";
import { getStarted } from "./getStarted";

const t = strings.dashboard.getStarted;

/**
 * The dashboard's *Get started* checklist (D-148; features/ui.md, Onboarding), for admins: each
 * step ticks itself from live state and links to where it is taken. Dismissed per browser (shown
 * again from the command palette or the help center, lib/checklist.ts); hidden
 * until every step can be told, so it never flickers through a wrong state.
 */
export function GetStarted(props: { nodes: readonly NodeSummary[]; graph: Graph | undefined }) {
  const live = useContext(LiveContext);
  const hidden = dismissed;
  const enabled = () => !hidden();
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
    enabled: enabled(),
  }));
  const backups = useQuery(() => ({
    queryKey: ["controller-backups"],
    queryFn: ({ signal }) => api.controller.backups.status(signal),
    enabled: enabled(),
  }));
  const notifiers = useQuery(() => ({
    queryKey: ["notifiers"],
    queryFn: ({ signal }) => api.notifiers.list(signal),
    enabled: enabled(),
  }));
  const steps = createMemo(() => {
    const config = site.data?.config;
    const wlans = config?.wlans?.length;
    const passphrase = backups.data?.passphrase_set;
    return getStarted({
      nodes: props.nodes,
      graph: props.graph,
      site: wlans === undefined ? undefined : { wlans, mesh: config?.mesh?.enabled === true },
      backups: typeof passphrase === "boolean" ? passphrase : undefined,
      notifiers: notifiers.data?.notifiers?.length,
    });
  });
  const done = () => steps()?.filter((step) => step.done).length ?? 0;
  const dismiss = () => setDismissed(true);

  return (
    <Show when={!hidden() && steps()}>
      {(list) => (
        <Card aria-label={t.title}>
          <CardHeader
            title={done() === list().length ? t.allDone : t.title}
            description={t.progress(done(), list().length)}
            actions={
              <Button
                size="sm"
                variant="ghost"
                icon={X}
                aria-label={t.dismiss}
                title={t.dismiss}
                onClick={dismiss}
              />
            }
          />
          <div class="px-4">
            <div
              role="progressbar"
              aria-label={t.title}
              aria-valuemin={0}
              aria-valuemax={list().length}
              aria-valuenow={done()}
              class="h-1 overflow-hidden rounded-full bg-raised"
            >
              <div
                class="h-full rounded-full bg-accent-fill transition-[width]"
                style={{ width: `${(100 * done()) / list().length}%` }}
              />
            </div>
          </div>
          <ol class="grid gap-2 p-4 pt-3 sm:grid-cols-2 xl:grid-cols-3">
            <For each={list()}>
              {(step) => {
                const text = () => t.steps[step.id];
                return (
                  <li
                    class="flex items-start gap-3 rounded-control border border-edge px-3 py-2.5"
                    aria-label={`${text().title}: ${step.done ? t.done : t.todo}`}
                  >
                    <Icon
                      icon={step.done ? CircleCheck : Circle}
                      class={["mt-0.5 size-4 shrink-0", step.done ? "text-ok" : "text-fg-subtle"]}
                    />
                    <div class="min-w-0 flex-1">
                      <p class={["font-medium text-sm", step.done && "text-fg-muted"]}>
                        {text().title}
                      </p>
                      <p
                        class={[
                          "mt-0.5 text-fg-subtle text-xs leading-relaxed",
                          step.done && "max-sm:hidden",
                        ]}
                      >
                        {step.progress && !step.done
                          ? t.managed(step.progress.done, step.progress.total)
                          : text().description}
                      </p>
                      <Show when={!step.done}>
                        <a
                          href={step.href}
                          class="mt-1.5 inline-flex items-center gap-1 font-medium text-accent text-xs hover:underline"
                        >
                          {text().action}
                          <Icon icon={ArrowRight} class="size-3" />
                        </a>
                      </Show>
                    </div>
                  </li>
                );
              }}
            </For>
          </ol>
        </Card>
      )}
    </Show>
  );
}
