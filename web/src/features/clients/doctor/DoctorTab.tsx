import { ArrowRight, CircleCheck, UserX } from "lucide";
import { createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { ClientDetail } from "../../../api/gen/ClientDetail";
import type { Diagnosis } from "../../../api/gen/Diagnosis";
import type { DoctorIssue } from "../../../api/gen/DoctorIssue";
import { clientName } from "../../../lib/clients";
import type { Names } from "../../../lib/events";
import { formatDateTime, formatDbm } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Badge, StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { Segmented } from "../../../ui/Segmented";
import { createMetricsFeed } from "../../../viz/timeseries/feed";
import { MetricChart } from "../../../viz/timeseries/MetricChart";
import type { Preset } from "../../../viz/timeseries/ranges";
import { TimeChart } from "../../../viz/timeseries/TimeChart";
import { liveWindow, sampleLine } from "../history";
import {
  DOCTOR_HOURS,
  type DoctorHours,
  entryText,
  evidenceText,
  fixText,
  severityTones,
} from "./describe";
import { createDiagnosis } from "./query";

const t = strings.client.doctor;
const PRESETS: Record<DoctorHours, Preset> = { 1: "1h", 24: "24h", 168: "7d" };
/** Timeline entries shown before *Show all*. */
const ENTRIES_SHOWN = 100;

/**
 * The Doctor tab (D-144): the issues found over the range with the fixes to try, then the signal
 * and one merged timeline of events, DHCP, Wi-Fi trouble and failed DNS lookups.
 */
export function DoctorTab(props: { detail: ClientDetail; onReconnect?: (() => void) | undefined }) {
  const live = useContext(LiveContext);
  const [hours, setHours] = createSignal<DoctorHours>(24);
  const mac = () => props.detail.client.mac;
  const diagnosis = createDiagnosis(mac, hours);
  const nodeName = (id: number) =>
    live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;
  return (
    <div class="flex flex-col gap-4">
      <div class="flex justify-end">
        <Segmented
          label={t.range}
          value={String(hours())}
          options={DOCTOR_HOURS.map((value) => ({ value: String(value), label: t.ranges[value] }))}
          onChange={(value) => setHours(Number(value) as DoctorHours)}
        />
      </div>
      <RequestError text={diagnosis.error?.message} />
      <Show
        when={diagnosis.isError ? undefined : diagnosis.data}
        fallback={
          <p role="status" class="py-10 text-center text-fg-subtle text-sm">
            {diagnosis.isError ? "" : `${t.loading}…`}
          </p>
        }
      >
        {(data) => (
          <>
            <Issues
              data={data()}
              span={t.ranges[hours()]}
              nodeName={nodeName}
              onReconnect={props.onReconnect}
            />
            <SignalCard detail={props.detail} hours={hours()} />
            <Timeline data={data()} nodeName={nodeName} client={clientName(props.detail.client)} />
          </>
        )}
      </Show>
    </div>
  );
}

function Issues(props: {
  data: Diagnosis;
  span: string;
  nodeName: (id: number) => string;
  onReconnect?: (() => void) | undefined;
}) {
  return (
    <Card aria-label={t.issues}>
      <CardHeader title={t.issues} description={t.issuesHint} />
      <Show
        when={props.data.issues.length > 0}
        fallback={
          <div class="flex items-center gap-3 px-4 pb-4">
            <Icon icon={CircleCheck} class="size-5 text-ok" />
            <div>
              <p class="font-medium text-sm">{t.healthy}</p>
              <p class="text-fg-muted text-sm">{t.healthyHint(props.span)}</p>
            </div>
          </div>
        }
      >
        <ul class="divide-y divide-edge border-edge border-t">
          <For each={props.data.issues}>
            {(issue) => (
              <IssueRow issue={issue} nodeName={props.nodeName} onReconnect={props.onReconnect} />
            )}
          </For>
        </ul>
      </Show>
      <Show when={!props.data.dns_logged}>
        <p class="border-edge border-t px-4 py-3 text-fg-muted text-xs">{t.noDns}</p>
      </Show>
    </Card>
  );
}

function IssueRow(props: {
  issue: DoctorIssue;
  nodeName: (id: number) => string;
  onReconnect?: (() => void) | undefined;
}) {
  const fixes = () => props.issue.fixes.map((fix) => fixText(fix, props.nodeName));
  return (
    <li class="flex flex-col gap-2 px-4 py-3">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
        <Badge tone={severityTones[props.issue.severity]}>{t.severity[props.issue.severity]}</Badge>
        <h3 class="font-medium text-sm">{t.problem[props.issue.problem.kind]}</h3>
        <span class="ml-auto text-fg-subtle text-xs">
          {t.lastSeen(formatDateTime(props.issue.at))}
        </span>
      </div>
      <p class="text-fg-muted text-sm">{evidenceText(props.issue.problem, props.nodeName)}</p>
      <Show when={fixes().length > 0}>
        <div class="flex flex-col gap-1.5">
          <span class="font-medium text-fg-subtle text-xs uppercase tracking-wide">
            {t.tryThis}
          </span>
          <ol class="flex flex-col gap-1.5">
            <For each={fixes()}>
              {(fix) => (
                <li class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
                  <span>{fix.text}</span>
                  <Show when={fix.link}>
                    {(link) => (
                      <a
                        href={link().href}
                        class="inline-flex items-center gap-1 rounded-control font-medium text-accent text-sm hover:underline"
                      >
                        {link().label}
                        <Icon icon={ArrowRight} class="size-3.5" />
                      </a>
                    )}
                  </Show>
                  <Show when={fix.reconnect && props.onReconnect}>
                    {(kick) => (
                      <Button size="sm" variant="ghost" icon={UserX} onClick={() => kick()()}>
                        {t.fix.kick}
                      </Button>
                    )}
                  </Show>
                </li>
              )}
            </For>
          </ol>
        </div>
      </Show>
    </li>
  );
}

/** The signal over the range: the samples in memory for the hour, the stored series beyond. */
function SignalCard(props: { detail: ClientDetail; hours: DoctorHours }) {
  const entity = () => `client:${props.detail.client.mac}`;
  const feed = createMetricsFeed(() =>
    props.hours === 1
      ? undefined
      : {
          queries: [{ entity: entity(), metrics: ["signal"] }],
          range: { preset: PRESETS[props.hours] },
          width: 800,
        },
  );
  const s = strings.client.signal;
  return (
    <Card class="p-4">
      <Show
        when={props.hours !== 1}
        fallback={
          <TimeChart
            title={t.signal}
            lines={[sampleLine(props.detail.samples, s.series.signal, "info", "signal_dbm")]}
            format={(v) => formatDbm(v)}
            minSpan={20}
            {...liveWindow(props.detail.samples)}
            empty={s.empty}
          />
        }
      >
        <MetricChart
          title={t.signal}
          feed={feed}
          lines={[{ entity: entity(), metric: "signal", label: s.series.signal }]}
          empty={s.stored}
        />
      </Show>
    </Card>
  );
}

function Timeline(props: { data: Diagnosis; nodeName: (id: number) => string; client: string }) {
  const tl = t.timeline;
  const [all, setAll] = createSignal(false);
  const names: Names = {
    node: (id) => props.nodeName(id),
    client: () => props.client,
    pattern: () => undefined,
  };
  const entries = createMemo(() =>
    props.data.timeline.map((entry) => ({ entry, text: entryText(entry, names) })),
  );
  const shown = () => (all() ? entries() : entries().slice(0, ENTRIES_SHOWN));
  return (
    <Card aria-label={tl.title}>
      <CardHeader title={tl.title} description={tl.hint} />
      <Show
        when={entries().length > 0}
        fallback={<p class="px-4 pb-6 text-center text-fg-subtle text-sm">{tl.empty}</p>}
      >
        <ol class="divide-y divide-edge border-edge border-t">
          <For each={shown()}>
            {(item) => (
              <li class="grid grid-cols-[auto_1fr] gap-x-3 px-4 py-2 sm:grid-cols-[11rem_auto_1fr]">
                <time
                  class="text-fg-subtle text-xs tabular-nums sm:pt-0.5"
                  datetime={new Date(item.entry.at).toISOString()}
                >
                  {formatDateTime(item.entry.at)}
                </time>
                <span class="row-span-2 flex items-start gap-2 sm:row-span-1 sm:pt-0.5">
                  <StatusDot tone={item.text.tone} class="mt-1.5 size-1.5" />
                  <span class="w-10 text-fg-subtle text-xs">{tl.kind[item.entry.kind]}</span>
                </span>
                <div class="col-span-2 min-w-0 sm:col-span-1">
                  <p class="break-words text-sm">{item.text.title}</p>
                  <Show
                    when={[
                      item.text.detail,
                      item.text.node === null ? null : props.nodeName(item.text.node as number),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  >
                    {(detail) => <p class="break-words text-fg-muted text-xs">{detail()}</p>}
                  </Show>
                </div>
              </li>
            )}
          </For>
        </ol>
        <Show when={!all() && entries().length > ENTRIES_SHOWN}>
          <div class="border-edge border-t px-4 py-2">
            <Button size="sm" variant="ghost" onClick={() => setAll(true)}>
              {tl.showAll(entries().length)}
            </Button>
          </div>
        </Show>
        <Show when={props.data.truncated}>
          <p class="border-edge border-t px-4 py-2 text-fg-subtle text-xs">
            {tl.truncated(props.data.timeline.length)}
          </p>
        </Show>
      </Show>
    </Card>
  );
}
