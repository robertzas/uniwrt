import type { JSX } from "@solidjs/web";
import type { UseQueryResult } from "@tanstack/solid-query";
import { Download } from "lucide";
import { createEffect, createMemo, createSignal, For, Show, untrack, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { OptionalToolStatus } from "../../../api/gen/OptionalToolStatus";
import { api, errorText } from "../../../lib/api";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { showToast } from "../../../ui/Toast";
import { concerns, endedCount } from "../software/software";

const t = strings.device.tools.optional;

/**
 * The tools OpenWrt's images lack (D-142): installed or not, and on a managed device an admin's
 * *Install*, the package job (D-139) with the package and what it replaces. In observe mode a
 * missing tool says why it can't be installed.
 */
export function OptionalToolsCard(props: {
  node: Node;
  admin: boolean;
  tools: UseQueryResult<OptionalToolStatus[]>;
}) {
  const note = () =>
    props.node.mode !== "managed" ? t.observe : !props.admin ? t.adminOnly : undefined;
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Show
        when={props.tools.data}
        fallback={
          <p
            role={props.tools.isError ? "alert" : "status"}
            class="px-4 pb-4 text-fg-subtle text-sm"
          >
            {props.tools.isError ? errorText(props.tools.error) : `${t.loading}…`}
          </p>
        }
      >
        {(tools) => (
          <Show
            when={props.admin && props.node.mode === "managed"}
            fallback={<Rows tools={tools()} action={() => undefined} />}
          >
            <Installer node={props.node} tools={tools()} refetch={() => props.tools.refetch()} />
          </Show>
        )}
      </Show>
      <Show when={props.tools.data?.some((tool) => !tool.installed) && note()}>
        {(why) => <p class="px-4 pb-4 text-fg-subtle text-xs">{why()}</p>}
      </Show>
    </Card>
  );
}

function Rows(props: {
  tools: readonly OptionalToolStatus[];
  action: (tool: OptionalToolStatus) => JSX.Element;
}) {
  return (
    <ul class="flex flex-col gap-2 px-4 pb-4">
      <For each={props.tools}>
        {(tool) => (
          <li class="flex min-h-8 items-center gap-3 text-sm">
            <span class="min-w-0 flex-1">
              <span class="block truncate">{t.tool[tool.tool]}</span>
              <span class="block truncate font-mono text-fg-subtle text-xs">{tool.package}</span>
            </span>
            <Show
              when={tool.installed}
              fallback={props.action(tool) ?? <Badge>{t.notInstalled}</Badge>}
            >
              <Badge tone="ok">{t.installed}</Badge>
            </Show>
          </li>
        )}
      </For>
    </ul>
  );
}

/** The rows with *Install*, following the device's packages jobs and reading the tools after each. */
function Installer(props: {
  node: Node;
  tools: readonly OptionalToolStatus[];
  refetch: () => unknown;
}) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const seen = new Set<number>();
  const ended = createMemo(() => endedCount(Object.values(jobs.byId), props.node.id, seen));
  const [own, setOwn] = createSignal<{ job: number; pkg: string }>();
  const [starting, setStarting] = createSignal<string>();
  const running = () =>
    Object.values(jobs.byId).some((job) => job.state === "running" && concerns(job, props.node.id));

  createEffect(ended, () => void props.refetch(), { defer: true });
  createEffect(
    () => {
      const mine = own();
      return mine ? jobs.byId[mine.job]?.state : undefined;
    },
    (state) => {
      const mine = untrack(own);
      if (!mine || state === undefined || state === "running") return;
      const job = untrack(() => jobs.byId[mine.job]);
      if (state === "succeeded") {
        showToast({ title: t.done(mine.pkg, props.node.name), tone: "ok" });
      } else {
        showToast({ title: t.failed(mine.pkg), description: job?.error?.message, tone: "crit" });
      }
      setOwn(undefined);
    },
  );

  const install = async (tool: OptionalToolStatus) => {
    setStarting(tool.package);
    try {
      const job = await api.nodes.changePackages(props.node.id, {
        install: [tool.package],
        remove: tool.replaces ? [tool.replaces] : [],
        upgrade: [],
      });
      addJobs([job]);
      setOwn({ job: job.id, pkg: tool.package });
    } catch (error) {
      showToast({ title: t.failed(tool.package), description: errorText(error), tone: "crit" });
    } finally {
      setStarting(undefined);
    }
  };

  return (
    <Rows
      tools={props.tools}
      action={(tool) => (
        <Button
          size="sm"
          icon={Download}
          disabled={running() || starting() !== undefined}
          title={t.installHint(tool.package, tool.replaces)}
          onClick={() => void install(tool)}
        >
          {starting() === tool.package || own()?.pkg === tool.package ? t.installing : t.install}
        </Button>
      )}
    />
  );
}
