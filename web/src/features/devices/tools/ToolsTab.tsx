import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal, useContext } from "solid-js";
import type { DiagRun } from "../../../api/gen/DiagRun";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import { api } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { CaptureCard } from "./CaptureCard";
import { OptionalToolsCard } from "./OptionalTools";
import { RunForm } from "./RunForm";
import { RunOutput } from "./RunOutput";
import { RunsCard } from "./RunsCard";
import { createRunView } from "./run";
import { TablesCard } from "./TablesCard";

/** How often the runs are read again: often while one runs, else now and then. */
const RUNNING_POLL_MS = 3_000;
const IDLE_POLL_MS = 15_000;

/**
 * A device's Tools tab (observability.md, Diagnostics toolkit; D-141, D-142): an admin starts a
 * run, everyone follows it live over `diag:<id>` and picks among the device's recent runs; an admin
 * captures packets; everyone reads the device's tables; the optional tools show whether they are
 * installed, with *Install* on a managed device.
 */
export function ToolsTab(props: { node: Node; facts: NodeFacts | null }) {
  const session = useContext(SessionContext);
  const live = useContext(LiveContext);
  const queryClient = useQueryClient();
  const admin = () => session.user.role === "admin";
  const runs = useQuery(() => ({
    queryKey: ["diag-runs"],
    queryFn: ({ signal }) => api.diag.runs(signal),
    refetchInterval: (query: { state: { data?: DiagRun[] } }) =>
      query.state.data?.some((run) => run.state === "running") ? RUNNING_POLL_MS : IDLE_POLL_MS,
  }));
  const tools = useQuery(() => ({
    queryKey: ["diag-tools", props.node.id],
    queryFn: ({ signal }) => api.diag.tools(props.node.id, signal),
    retry: false,
  }));
  const mine = createMemo(() => (runs.data ?? []).filter((run) => run.node === props.node.id));
  const [picked, setPicked] = createSignal<number>();
  const shown = () => picked() ?? mine()[0]?.id;
  const view = createRunView(live, shown);
  const summary = () => live.fleet.nodes.find((node) => node.id === props.node.id);
  const online = () => summary()?.status.state !== "offline";
  const name = (id: number) => live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;

  // The list's copy of a run that ended is stale. Read it again after the callback: invalidating
  // inside it would re-render the list within the effect (STRICT_READ_UNTRACKED).
  createEffect(
    () => view.run?.state,
    (state) => {
      if (state !== undefined && state !== "running") {
        queueMicrotask(() => void queryClient.invalidateQueries({ queryKey: ["diag-runs"] }));
      }
    },
    { defer: true },
  );

  const started = (run: DiagRun) => {
    queryClient.setQueryData<DiagRun[]>(["diag-runs"], (old) => [
      run,
      ...(old ?? []).filter((other) => other.id !== run.id),
    ]);
    setPicked(run.id);
  };

  return (
    <div class="grid items-start gap-4 lg:grid-cols-5">
      <div class="flex min-w-0 flex-col gap-4 lg:col-span-2">
        <RunForm
          node={props.node}
          facts={props.facts}
          admin={admin()}
          online={online()}
          tools={tools.data}
          onStarted={started}
        />
        <CaptureCard
          node={props.node}
          facts={props.facts}
          admin={admin()}
          online={online()}
          tools={tools.data}
        />
        <OptionalToolsCard node={props.node} admin={admin()} tools={tools} />
      </div>
      <div class="flex min-w-0 flex-col gap-4 lg:col-span-3">
        <RunOutput
          view={view}
          shown={shown() !== undefined}
          nodeName={props.node.name}
          name={name}
          admin={admin()}
        />
        <RunsCard runs={mine()} selected={shown()} onSelect={setPicked} name={name} />
      </div>
      <div class="min-w-0 lg:col-span-5">
        <TablesCard node={props.node.id} />
      </div>
    </div>
  );
}
