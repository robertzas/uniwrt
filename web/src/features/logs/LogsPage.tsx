import { useSearchParams } from "@solidjs/router";
import { ArrowUp, Clock, Pause, Play, RotateCw, ScrollText } from "lucide";
import { createEffect, createMemo, createSignal, onSettled, Show, useContext } from "solid-js";
import type { LogPattern } from "../../api/gen/LogPattern";
import type { LogRecord } from "../../api/gen/LogRecord";
import type { LogSources } from "../../api/gen/LogSources";
import { api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { StatusDot } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { ContextDialog } from "./ContextDialog";
import { barsFor, Histogram } from "./Histogram";
import { createHistogramFeed } from "./histogram";
import { LogList } from "./LogList";
import { LogRow, type SourceLabel } from "./LogRow";
import { parseText } from "./match";
import type { PickerGroup } from "./Picker";
import {
  EMPTY_FILTER,
  type Filter,
  filterParams,
  isFiltered,
  type LogParams,
  type LogRange,
  parseParams,
  rangeParams,
} from "./query";
import { Toolbar } from "./Toolbar";
import { createLogTail, MAX_RECORDS } from "./tail";

const t = strings.logs;

/**
 * `/insights/logs`: every source's log, searchable and live. The filters and the range live in
 * the URL (`?nodes=3&min=warning&q=…&range=1h`), so a view can be shared and the device page
 * links here. A preset range follows new entries; dragging across the histogram fixes a window.
 */
export function LogsPage() {
  const live = useContext(LiveContext);
  const [params, setParams] = useSearchParams<LogParams>();
  const query = createMemo(() =>
    parseParams({
      nodes: params.nodes,
      hosts: params.hosts,
      programs: params.programs,
      min: params.min,
      q: params.q,
      annotated: params.annotated,
      range: params.range,
      from: params.from,
      to: params.to,
    }),
  );
  const filter = () => query().filter;
  const tail = createLogTail(live, query);
  const [bars, setBars] = createSignal(60);
  const histogram = createHistogramFeed(query, bars);
  const patterns = createPatterns();
  const [sources, readSources] = createSources();
  const [expanded, setExpanded] = createSignal<ReadonlySet<number>>(new Set());
  const [context, setContext] = createSignal<LogRecord>();
  let scroll: { toTop: () => void } | undefined;
  const terms = createMemo(() => (filter().text === null ? [] : parseText(filter().text ?? "")));

  const setFilter = (next: Filter) => setParams(filterParams(next));
  const setRange = (range: LogRange) => setParams(rangeParams(range));
  const sourceOf = (record: LogRecord): SourceLabel => {
    if (record.node !== null) {
      const node = live.fleet.nodes.find((n) => n.id === record.node);
      return { name: node?.name ?? t.node(record.node), href: `/devices/${record.node}` };
    }
    return { name: record.host ?? t.unknownSource };
  };
  const sourceGroups = createMemo((): PickerGroup[] => {
    const named = new Set(filter().hosts);
    const hosts = [...new Set([...(sources()?.hosts ?? []), ...named])].sort();
    return [
      {
        label: t.nodes,
        options: live.fleet.nodes.map((node) => ({
          value: `node:${node.id}`,
          label: node.name,
          hint: node.host,
        })),
      },
      { label: t.hosts, options: hosts.map((host) => ({ value: `host:${host}`, label: host })) },
    ];
  });
  const toggle = (id: number) =>
    setExpanded((now) => {
      const next = new Set(now);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const showNew = () => {
    scroll?.toTop();
    tail.setFollowing(true);
    tail.setPaused(false);
  };

  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  const row = (record: LogRecord) => (
    <LogRow
      record={record}
      source={sourceOf(record)}
      pattern={patterns().get(record.pattern ?? "")}
      terms={terms()}
      expanded={expanded().has(record.id)}
      onToggle={() => toggle(record.id)}
      onContext={() => setContext(record)}
      onOnlySource={() =>
        setFilter({
          ...filter(),
          nodes: record.node === null ? [] : [record.node],
          hosts: record.node === null && record.host !== null ? [record.host] : [],
        })
      }
      onOnlyProgram={() =>
        setFilter({ ...filter(), programs: record.program === null ? [] : [record.program] })
      }
    />
  );

  return (
    <div class="mx-auto flex max-w-7xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{strings.nav.logs.description}</p>
      </header>
      <Card class="flex flex-col gap-4 p-4">
        <Toolbar
          filter={filter()}
          range={query().range}
          sourceGroups={sourceGroups()}
          programs={sources()?.programs ?? []}
          onFilter={setFilter}
          onRange={setRange}
          onSources={readSources}
        />
        <Histogram
          data={histogram.data()}
          failed={histogram.failed()}
          onWidth={(width) => setBars(barsFor(width))}
          onZoom={(from, to) => setRange({ from, to })}
        />
      </Card>
      <Card class="overflow-hidden" aria-label={t.list}>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-2 border-edge border-b px-4 py-2.5">
          <TailState
            live={tail.live()}
            paused={tail.paused()}
            following={tail.following()}
            count={tail.records().length}
          />
          <div class="ml-auto flex items-center gap-2">
            <Show when={tail.held() > 0}>
              <Button size="sm" variant="primary" icon={ArrowUp} onClick={showNew}>
                {t.showNew(tail.held())}
              </Button>
            </Show>
            <Show when={tail.live()}>
              <Button
                size="sm"
                variant="ghost"
                icon={tail.paused() ? Play : Pause}
                onClick={() => tail.setPaused(!tail.paused())}
              >
                {tail.paused() ? t.resume : t.pause}
              </Button>
            </Show>
          </div>
        </div>
        <LogList
          records={tail.records()}
          row={row}
          onFollow={(following) => tail.setFollowing(following)}
          onEnd={() => tail.loadOlder()}
          controls={(controls) => {
            scroll = controls;
          }}
          footer={
            <Footer
              older={tail.olderStatus()}
              hasOlder={tail.hasOlder()}
              capped={tail.capped()}
              onMore={() => tail.loadOlder()}
            />
          }
          empty={
            <Show
              when={tail.status() !== "loading"}
              fallback={
                <p role="status" class="py-20 text-center text-fg-subtle text-sm">
                  {t.loading}…
                </p>
              }
            >
              <Show
                when={tail.status() !== "failed"}
                fallback={
                  <div role="alert" class="flex flex-col items-center gap-3 py-16 text-sm">
                    <p class="text-fg-muted">{t.failed}</p>
                    <Button size="sm" icon={RotateCw} onClick={() => tail.retry()}>
                      {t.retry}
                    </Button>
                  </div>
                }
              >
                <div class="py-6">
                  <EmptyState
                    icon={ScrollText}
                    title={t.empty.title}
                    description={isFiltered(filter()) ? t.empty.filtered : t.empty.live}
                  />
                  <Show when={isFiltered(filter())}>
                    <div class="mt-2 flex justify-center">
                      <Button size="sm" onClick={() => setFilter(EMPTY_FILTER)}>
                        {t.clear}
                      </Button>
                    </div>
                  </Show>
                </div>
              </Show>
            </Show>
          }
        />
      </Card>
      <ContextDialog
        record={context()}
        onClose={() => setContext(undefined)}
        source={sourceOf}
        pattern={(id) => (id === null ? undefined : patterns().get(id))}
      />
    </div>
  );
}

function TailState(props: { live: boolean; paused: boolean; following: boolean; count: number }) {
  const label = () => {
    if (!props.live) return t.state.window;
    if (props.paused) return t.state.paused;
    return props.following ? t.state.live : t.state.held;
  };
  return (
    <p role="status" class="flex items-center gap-2 text-sm">
      <Show when={props.live} fallback={<Icon icon={Clock} class="size-3.5 text-fg-subtle" />}>
        <StatusDot
          tone={props.paused || !props.following ? "warn" : "ok"}
          pulse={!props.paused && props.following}
        />
      </Show>
      <span class="font-medium text-fg">{label()}</span>
      <span class="text-fg-subtle tabular-nums">· {t.count(props.count)}</span>
    </p>
  );
}

function Footer(props: {
  older: "idle" | "loading" | "failed";
  hasOlder: boolean;
  capped: boolean;
  onMore: () => void;
}) {
  return (
    <div class="flex justify-center px-4 py-3 text-fg-subtle text-xs">
      <Show
        when={props.older !== "loading"}
        fallback={<span role="status">{t.loadingOlder}…</span>}
      >
        <Show
          when={props.hasOlder}
          fallback={<span>{props.capped ? t.capped(MAX_RECORDS) : t.end}</span>}
        >
          <div class="flex items-center gap-2">
            <Show when={props.older === "failed"}>
              <span role="alert">{t.olderFailed}</span>
            </Show>
            <Button size="sm" variant="ghost" onClick={() => props.onMore()}>
              {t.loadOlder}
            </Button>
          </div>
        </Show>
      </Show>
    </div>
  );
}

/** What each annotation means, by id; read once when the page opens. */
function createPatterns(): () => Map<string, LogPattern> {
  const [patterns, setPatterns] = createSignal(new Map<string, LogPattern>());
  onSettled(() => {
    const controller = new AbortController();
    api.logs.patterns(controller.signal).then(
      (answer) => setPatterns(new Map(answer.patterns.map((p) => [p.id, p]))),
      () => {
        // Annotated entries show without their explanation.
      },
    );
    return () => controller.abort();
  });
  return patterns;
}

/** The hosts and programs that logged lately: read when the page opens and a picker does. */
function createSources(): [() => LogSources | undefined, () => void] {
  const [sources, setSources] = createSignal<LogSources>();
  const controller = new AbortController();
  const read = () => {
    api.logs.sources(controller.signal).then(
      (answer) => setSources(answer.sources),
      () => {
        // The pickers offer the devices and what is chosen.
      },
    );
  };
  onSettled(() => {
    read();
    return () => controller.abort();
  });
  return [sources, read];
}
