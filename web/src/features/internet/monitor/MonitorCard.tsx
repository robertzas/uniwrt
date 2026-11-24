import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { Activity, CircleAlert } from "lucide";
import { createMemo, createSignal, onCleanup, Show } from "solid-js";
import { api } from "../../../lib/api";
import { formatDuration, formatTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { type Column, Table } from "../../../ui/Table";
import { showToast } from "../../../ui/Toast";
import { formatUnit } from "../../../viz/timeseries/units";
import { faultOf, healthOf, lossTone, type ProbeRow, rowsOf } from "./monitor";
import { Outages } from "./Outages";
import { ProbeChart } from "./ProbeChart";
import { TargetsDialog } from "./TargetsDialog";

const t = strings.internet.monitor;
/** The gateway probes every 30 s: the view is read as often. */
const POLL_MS = 30_000;
/** About how long a round takes: 10 pings a second apart, and the lookups beside them. */
const ROUND_MS = 15_000;
const ms = formatUnit("milliseconds");
const percent = formatUnit("percent");

/**
 * The WAN quality monitor on `/internet` (observability.md, Troubleshooting assistants; D-144):
 * the internet's health and where an outage lies, the latest round of probes, a target's latency
 * band and loss over a range, the outages, and for admins the targets' editor.
 */
export function MonitorCard(props: { admin: boolean; now: number }) {
  const client = useQueryClient();
  const monitor = useQuery(() => ({
    queryKey: ["wan-monitor"],
    queryFn: ({ signal }) => api.wan.monitor(signal),
    refetchInterval: POLL_MS,
  }));
  const [editing, setEditing] = createSignal(false);
  const [picked, setPicked] = createSignal<string>();
  let refetch: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(refetch));
  const rows = createMemo(() => (monitor.data ? rowsOf(monitor.data) : []));
  /** The charted series: the one picked while the round still has it, else the first. */
  const chosen = () => {
    const key = picked();
    return rows().find((row) => row.key === key) ?? rows()[0];
  };
  const since = () => {
    const at = monitor.data?.since;
    return at === null || at === undefined
      ? undefined
      : formatDuration(Math.max(0, (Date.now() - at) / 1000));
  };

  const columns: Column<ProbeRow>[] = [
    {
      id: "target",
      header: t.columns.target,
      cell: (row) => (
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="truncate font-medium">{row.name}</span>
            <Show when={row.kind}>
              {(kind) => <span class="text-fg-subtle text-xs">{t.kinds[kind()]}</span>}
            </Show>
          </div>
          <div class="truncate text-fg-subtle text-xs">{row.detail}</div>
        </div>
      ),
    },
    {
      id: "latency",
      header: t.columns.latency,
      align: "end",
      class: "w-28",
      cell: (row) =>
        row.median === null ? (
          <span class="text-fg-muted" title={row.error ?? undefined}>
            {t.lost}
          </span>
        ) : (
          ms(row.median)
        ),
      sort: (a, b) =>
        (a.median ?? Number.POSITIVE_INFINITY) - (b.median ?? Number.POSITIVE_INFINITY),
    },
    {
      id: "band",
      header: t.columns.band,
      align: "end",
      class: "w-40",
      cell: (row) =>
        row.min === null || row.max === null || row.kind !== "icmp"
          ? "—"
          : `${ms(row.min)} – ${ms(row.max)}`,
    },
    {
      id: "loss",
      header: t.columns.loss,
      align: "end",
      class: "w-24",
      cell: (row) => (
        <Badge tone={lossTone(row.loss)}>{row.loss === null ? "—" : percent(row.loss)}</Badge>
      ),
      sort: (a, b) => (a.loss ?? 101) - (b.loss ?? 101),
    },
  ];

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && monitor.data}>
            <Button size="sm" onClick={() => setEditing(true)}>
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-4 px-4 pb-4">
        <Show when={monitor.data} fallback={<p class="text-fg-muted text-sm">{t.loading}</p>}>
          {(view) => (
            <>
              <Show
                when={view().settings.enabled}
                fallback={<p class="text-fg-muted text-sm">{t.off}</p>}
              >
                <div class="flex flex-col gap-1.5 text-sm">
                  <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
                    <Badge tone={healthOf(view()).tone} icon={Activity}>
                      <span class="font-medium">{healthOf(view()).label}</span>
                      <Show when={view().health && since()}>
                        {(d) => <span class="text-fg-muted"> {t.since(d())}</span>}
                      </Show>
                    </Badge>
                    <Show when={view().round}>
                      {(round) => (
                        <span class="text-fg-subtle text-xs">
                          {t.checked(formatTime(round().at))} ·{" "}
                          {round().first_hop
                            ? t.firstHop(round().first_hop ?? "")
                            : !round().route
                              ? t.noRoute
                              : ""}
                        </span>
                      )}
                    </Show>
                  </div>
                  <Show when={faultOf(view())}>
                    {(fault) => (
                      <p class="text-fg">
                        {t.fault[fault()]}
                        <Show when={view().last_hop}>
                          {(hop) => <span class="text-fg-muted"> {t.lastHop(hop())}</span>}
                        </Show>
                      </p>
                    )}
                  </Show>
                  <Show when={view().error}>
                    {(error) => (
                      <p class="flex items-start gap-1.5 text-fg-muted">
                        <Icon icon={CircleAlert} class="mt-0.5 size-3.5 shrink-0 text-warn" />
                        {t.error(error())}
                      </p>
                    )}
                  </Show>
                </div>
                <Show
                  when={rows().length > 0}
                  fallback={<p class="text-fg-muted text-sm">{t.noRound}</p>}
                >
                  <Table
                    label={t.table}
                    columns={columns}
                    rows={rows()}
                    rowKey={(row) => row.key}
                    onRowClick={(row) => setPicked(row.key)}
                  />
                </Show>
              </Show>
              <Show when={chosen()}>
                {(row) => <ProbeChart rows={rows()} chosen={row()} onChoose={setPicked} />}
              </Show>
              <Outages now={props.now} />
            </>
          )}
        </Show>
        <RequestError text={monitor.error?.message} />
      </div>
      <Show when={editing() && monitor.data}>
        {(view) => (
          <TargetsDialog
            view={view()}
            onClose={() => setEditing(false)}
            onSaved={(next) => {
              client.setQueryData(["wan-monitor"], next);
              setEditing(false);
              showToast({ title: t.editor.saved, tone: "ok" });
              // The round the save started lands in a few seconds.
              clearTimeout(refetch);
              refetch = setTimeout(() => void monitor.refetch(), ROUND_MS);
            }}
          />
        )}
      </Show>
    </Card>
  );
}
