import { For, Show } from "solid-js";
import type { DiagRun } from "../../../api/gen/DiagRun";
import { formatTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { StatusDot } from "../../../ui/Badge";
import { Card, CardHeader } from "../../../ui/Card";
import { describeTool } from "./draft";
import { RUN_TONE } from "./run";

const t = strings.device.tools;

/** The device's runs the controller keeps, newest first; picking one shows it. */
export function RunsCard(props: {
  runs: readonly DiagRun[];
  selected: number | undefined;
  onSelect: (id: number) => void;
  name: (node: number) => string;
}) {
  return (
    <Card aria-label={t.runs.title}>
      <CardHeader title={t.runs.title} />
      <Show
        when={props.runs.length > 0}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.runs.empty}</p>}
      >
        <ul aria-label={t.runs.label} class="flex flex-col px-2 pb-2">
          <For each={props.runs}>
            {(run) => (
              <li>
                <button
                  type="button"
                  aria-current={props.selected === run.id ? "true" : undefined}
                  onClick={() => props.onSelect(run.id)}
                  class="flex w-full items-center gap-2.5 rounded-control px-2 py-1.5 text-left text-sm hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent aria-[current]:bg-raised"
                >
                  <StatusDot tone={RUN_TONE[run.state]} pulse={run.state === "running"} />
                  <span class="min-w-0 flex-1 truncate">{describeTool(run.tool, props.name)}</span>
                  <span class="shrink-0 text-fg-subtle text-xs">{t.state[run.state]}</span>
                  <span class="w-20 shrink-0 text-right text-fg-subtle text-xs tabular-nums">
                    {formatTime(run.started_at)}
                  </span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </Card>
  );
}
