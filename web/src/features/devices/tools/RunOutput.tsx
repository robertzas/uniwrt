import { Square } from "lucide";
import { createEffect, createMemo, createSignal, For, Match, Show, Switch } from "solid-js";
import type { DiagRun } from "../../../api/gen/DiagRun";
import { api, errorText } from "../../../lib/api";
import { formatBitrate, formatBytes, formatDuration, formatTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Sparkline } from "../../../ui/Sparkline";
import { showToast } from "../../../ui/Toast";
import { describeTool } from "./draft";
import { type Flow, iperf3Flows } from "./iperf3";
import { RUN_TONE, type RunViewState, runSeconds } from "./run";

const t = strings.device.tools;

/**
 * The run shown: what it runs, how it stands, its output as it streams (kept scrolled to the end
 * unless scrolled up), why it failed, and for a throughput test each direction's rate. Admins
 * stop it while it runs.
 */
export function RunOutput(props: {
  view: RunViewState;
  /** A run is picked. */
  shown: boolean;
  nodeName: string;
  name: (node: number) => string;
  admin: boolean;
}) {
  return (
    <Card
      class="min-w-0"
      aria-label={
        props.shown && props.view.run
          ? describeTool(props.view.run.tool, props.name)
          : t.output.title
      }
    >
      <Switch
        fallback={
          <>
            <CardHeader title={t.output.title} />
            <p class="px-4 pb-6 text-fg-subtle text-sm">{t.output.empty}</p>
          </>
        }
      >
        <Match when={props.shown && !props.view.ready}>
          <CardHeader title={t.output.title} />
          <p role="status" class="px-4 pb-6 text-fg-subtle text-sm">
            {t.output.loading}…
          </p>
        </Match>
        <Match when={props.shown && props.view.missing}>
          <CardHeader title={t.output.title} />
          <p class="px-4 pb-6 text-fg-subtle text-sm">{t.output.gone}</p>
        </Match>
        <Match when={props.shown && props.view.run}>
          {(run) => (
            <Shown run={run()} nodeName={props.nodeName} name={props.name} admin={props.admin} />
          )}
        </Match>
      </Switch>
    </Card>
  );
}

function Shown(props: {
  run: DiagRun;
  nodeName: string;
  name: (node: number) => string;
  admin: boolean;
}) {
  const [stopping, setStopping] = createSignal(false);
  const running = () => props.run.state === "running";
  const stop = async () => {
    setStopping(true);
    try {
      await api.diag.stop(props.run.id);
    } catch (error) {
      showToast({ title: t.output.stopFailed, description: errorText(error), tone: "crit" });
    } finally {
      setStopping(false);
    }
  };
  const meta = () => {
    const started = t.output.meta(formatTime(props.run.started_at), props.run.by);
    return props.run.ended_at === null
      ? started
      : `${started} · ${t.output.took(formatDuration(runSeconds(props.run, Date.now())))}`;
  };
  const flows = createMemo(() => {
    const tool = props.run.tool;
    if (tool.tool !== "iperf3") return [];
    return iperf3Flows(
      props.run.lines.map((line) => line.text),
      tool,
    );
  });
  const peer = () => {
    const tool = props.run.tool;
    if (tool.tool !== "iperf3") return "";
    return tool.peer === "controller" ? t.iperf3.controller : props.name(tool.peer.node);
  };
  return (
    <>
      <CardHeader
        title={describeTool(props.run.tool, props.name)}
        description={meta()}
        actions={
          <div class="flex items-center gap-2">
            <Badge tone={RUN_TONE[props.run.state]}>{t.state[props.run.state]}</Badge>
            <Show when={props.admin && running()}>
              <Button size="sm" icon={Square} disabled={stopping()} onClick={() => void stop()}>
                {t.output.stop}
              </Button>
            </Show>
          </div>
        }
      />
      <Show when={flows().length > 0}>
        <div class="grid gap-3 px-4 pb-3 sm:grid-cols-2">
          <For each={flows()}>
            {(flow) => (
              <FlowStat flow={flow} node={props.nodeName} peer={peer()} running={running()} />
            )}
          </For>
        </div>
      </Show>
      <Lines run={props.run} />
      <Show
        when={
          props.run.error ?? (props.run.exit_code ? t.output.exit(props.run.exit_code) : undefined)
        }
      >
        {(why) => (
          <p
            role={props.run.state === "failed" ? "alert" : undefined}
            class="px-4 pb-4 text-crit text-sm"
          >
            {why()}
          </p>
        )}
      </Show>
    </>
  );
}

/** The output, scrolled to its end as lines arrive unless the reader scrolled up. */
function Lines(props: { run: DiagRun }) {
  const [pane, setPane] = createSignal<HTMLPreElement>();
  let pinned = true;
  createEffect(
    () => [pane(), props.run.lines.length] as const,
    ([element]) => {
      if (element && pinned) element.scrollTop = element.scrollHeight;
    },
  );
  return (
    <div class="px-4 pb-4">
      <Show when={props.run.dropped > 0}>
        <p class="pb-1 text-fg-subtle text-xs">{t.output.dropped(props.run.dropped)}</p>
      </Show>
      <pre
        ref={setPane}
        tabindex="0"
        role="log"
        aria-label={t.output.label}
        aria-live="off"
        onScroll={(event) => {
          const el = event.currentTarget;
          pinned = el.scrollTop + el.clientHeight >= el.scrollHeight - 8;
        }}
        class="max-h-[28rem] min-h-24 overflow-auto rounded-control border border-edge bg-canvas p-3 font-mono text-fg text-xs leading-5 focus-visible:outline-2 focus-visible:outline-accent"
      >
        <Show
          when={props.run.lines.length > 0}
          fallback={
            <span class="text-fg-subtle">
              {props.run.state === "running" ? t.output.waiting : t.output.none}
            </span>
          }
        >
          <For each={props.run.lines}>
            {(line) => <div class={{ "text-warn": line.err }}>{line.text || " "}</div>}
          </For>
        </Show>
      </pre>
    </div>
  );
}

/** One direction of a throughput test: what arrived (so far, while it runs), each second's rate. */
function FlowStat(props: { flow: Flow; node: string; peer: string; running: boolean }) {
  const i = t.iperf3;
  const rate = () => props.flow.receiver?.bps ?? props.flow.intervals.at(-1) ?? null;
  return (
    <div class="flex min-w-0 flex-col gap-1 rounded-control border border-edge px-3 py-2.5">
      <span class="truncate text-fg-subtle text-xs">
        {props.flow.way === "up" ? i.up(props.node, props.peer) : i.down(props.node, props.peer)}
      </span>
      <span class="font-semibold text-xl tabular-nums">{formatBitrate(rate())}</span>
      <span class="text-fg-subtle text-xs">{props.flow.receiver ? i.received : i.soFar}</span>
      <Sparkline values={props.flow.intervals} label={i.trend} class="text-accent" />
      <Show when={props.flow.sender}>
        {(sender) => (
          <span class="text-fg-subtle text-xs tabular-nums">
            {[
              i.sent(formatBitrate(sender().bps), formatBytes(sender().bytes)),
              ...(sender().retransmits === null ? [] : [i.retransmits(sender().retransmits ?? 0)]),
            ].join(" · ")}
          </span>
        )}
      </Show>
    </div>
  );
}
