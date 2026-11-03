import { History, RotateCw } from "lucide";
import { For, Show } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { Names } from "../../lib/alerts";
import { isLevel, LEVELS } from "../../lib/events";
import type { Live } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Segmented } from "../../ui/Segmented";
import { AlertRow } from "./AlertRow";
import { createAlertHistory, type HistoryQuery, type HistoryRange, RANGES } from "./history";

const t = strings.alerts.history;

const TOOL_SELECT =
  "h-8 cursor-pointer rounded-control border bg-surface pr-7 pl-2.5 text-sm focus-visible:outline-2 focus-visible:outline-accent";

/** Every alert that started in a range, newest first, live; older ones a page at a time. */
export function HistoryTab(props: {
  live: Live;
  names: Names;
  now: number;
  admin: boolean;
  query: HistoryQuery;
  onQuery: (query: HistoryQuery) => void;
  expanded: ReadonlySet<number>;
  onToggle: (id: number) => void;
}) {
  const history = createAlertHistory(props.live, () => props.query);
  const filtered = () => props.query.min !== null || props.query.node !== null;
  const set = (change: Partial<HistoryQuery>) => props.onQuery({ ...props.query, ...change });

  return (
    <Card class="overflow-hidden" aria-label={t.label}>
      <div class="flex flex-wrap items-center gap-2 border-edge border-b px-4 py-2.5">
        <p role="status" class="mr-auto text-sm">
          <span class="font-medium text-fg tabular-nums">{t.count(history.alerts().length)}</span>
        </p>
        <label class="sr-only" for="alert-history-level">
          {t.level}
        </label>
        <select
          id="alert-history-level"
          value={props.query.min ?? ""}
          onChange={(event) => {
            const value = event.currentTarget.value;
            set({ min: isLevel(value) ? value : null });
          }}
          class={[
            TOOL_SELECT,
            props.query.min ? "border-accent/50 text-fg" : "border-edge text-fg-muted",
          ]}
        >
          <option value="">{t.anyLevel}</option>
          <For each={LEVELS.slice(0, 3)}>
            {(level) => (
              <option value={level}>
                {strings.events.atLeast(strings.events.levelName[level])}
              </option>
            )}
          </For>
        </select>
        <label class="sr-only" for="alert-history-device">
          {t.device}
        </label>
        <select
          id="alert-history-device"
          value={props.query.node === null ? "" : String(props.query.node)}
          onChange={(event) => {
            const value = Number(event.currentTarget.value);
            set({ node: value > 0 ? value : null });
          }}
          class={[
            TOOL_SELECT,
            props.query.node ? "border-accent/50 text-fg" : "border-edge text-fg-muted",
          ]}
        >
          <option value="">{t.anyDevice}</option>
          <For each={props.live.fleet.nodes}>
            {(node: NodeSummary) => <option value={String(node.id)}>{node.name}</option>}
          </For>
        </select>
        <Segmented
          label={t.range}
          value={props.query.range}
          options={RANGES.map((range) => ({
            value: range,
            label: t.ranges[range][0],
            description: t.ranges[range][1],
          }))}
          onChange={(range: HistoryRange) => set({ range })}
        />
      </div>
      <Show
        when={history.alerts().length > 0}
        fallback={
          <Show
            when={history.status() !== "loading"}
            fallback={
              <p role="status" class="py-20 text-center text-fg-subtle text-sm">
                {t.loading}…
              </p>
            }
          >
            <Show
              when={history.status() !== "failed"}
              fallback={
                <div role="alert" class="flex flex-col items-center gap-3 py-16 text-sm">
                  <p class="text-fg-muted">{t.failed}</p>
                  <Button size="sm" icon={RotateCw} onClick={() => history.retry()}>
                    {t.retry}
                  </Button>
                </div>
              }
            >
              <div class="py-6">
                <EmptyState
                  icon={History}
                  title={t.emptyTitle}
                  description={filtered() ? t.filtered : t.empty}
                />
                <Show when={filtered()}>
                  <div class="mt-2 flex justify-center">
                    <Button size="sm" onClick={() => set({ min: null, node: null })}>
                      {t.clear}
                    </Button>
                  </div>
                </Show>
              </div>
            </Show>
          </Show>
        }
      >
        <ul>
          <For each={history.alerts()}>
            {(alert) => (
              <AlertRow
                alert={alert}
                names={props.names}
                now={props.now}
                admin={props.admin}
                expanded={props.expanded.has(alert.id)}
                onToggle={() => props.onToggle(alert.id)}
              />
            )}
          </For>
        </ul>
        <div class="flex justify-center px-4 py-3 text-fg-subtle text-xs">
          <Show
            when={history.olderStatus() !== "loading"}
            fallback={<span role="status">{t.loadingOlder}…</span>}
          >
            <Show when={history.hasOlder()} fallback={<span>{t.end}</span>}>
              <div class="flex items-center gap-2">
                <Show when={history.olderStatus() === "failed"}>
                  <span role="alert">{t.olderFailed}</span>
                </Show>
                <Button size="sm" variant="ghost" onClick={() => history.loadOlder()}>
                  {t.loadOlder}
                </Button>
              </div>
            </Show>
          </Show>
        </div>
      </Show>
    </Card>
  );
}
