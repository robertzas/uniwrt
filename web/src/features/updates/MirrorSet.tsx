import { ChevronRight } from "lucide";
import { For, Show } from "solid-js";
import type { FeedStatus } from "../../api/gen/FeedStatus";
import type { SetStatus } from "../../api/gen/SetStatus";
import { formatBytes, formatDateTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Icon } from "../../ui/Icon";
import { type Column, Table } from "../../ui/Table";
import { failingFeeds, feedName, feedPath, setLabel } from "./mirror";

const t = strings.firmware.mirror;

const columns: Column<FeedStatus>[] = [
  {
    id: "feed",
    header: t.columns.feed,
    class: "min-w-48",
    cell: (feed) => (
      <span class="flex min-w-0 flex-col">
        <span class="font-medium">{feedName(feed.url)}</span>
        <span class="truncate font-mono text-fg-subtle text-xs" title={feed.url}>
          {feedPath(feed.url)}
        </span>
      </span>
    ),
  },
  {
    id: "index",
    header: t.columns.index,
    cell: (feed) => (
      <span class="whitespace-nowrap text-fg-muted text-xs">
        {feed.index_at === null ? t.noIndex : formatDateTime(feed.index_at)}
      </span>
    ),
  },
  {
    id: "packages",
    header: t.columns.packages,
    align: "end",
    cell: (feed) => `${feed.kept} / ${feed.listed}`,
  },
  { id: "size", header: t.columns.size, align: "end", cell: (feed) => formatBytes(feed.bytes) },
  {
    id: "state",
    header: t.columns.state,
    class: "max-w-72",
    cell: (feed) => (
      <Show when={feed.error} fallback={<Badge tone="ok">{t.ok}</Badge>}>
        {(error) => (
          <span class="flex flex-col items-start gap-0.5 whitespace-normal">
            <Badge tone="crit">{t.error}</Badge>
            <span class="break-words text-fg-muted text-xs">{error()}</span>
          </span>
        )}
      </Show>
    ),
  },
];

/**
 * One feed set of the mirror, folded: its release, target, architecture and package manager,
 * whether a device runs it or it is kept for an upgrade on offer, its packages and size, and
 * how many feeds fail; unfolded, each feed's index time, counts, size and error, the wanted
 * names no feed has, and the packages that couldn't be kept. Opens by itself when something
 * fails.
 */
export function MirrorSet(props: { status: SetStatus }) {
  const failing = () => failingFeeds(props.status);
  const troubled = () =>
    failing() > 0 || props.status.missing.length > 0 || props.status.failed.length > 0;
  return (
    <details class="group rounded-control border border-edge" open={troubled()}>
      <summary class="flex cursor-pointer list-none flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 [&::-webkit-details-marker]:hidden">
        <Icon
          icon={ChevronRight}
          class="size-4 shrink-0 text-fg-subtle transition-transform group-open:rotate-90"
        />
        <span class="font-medium tabular-nums">{setLabel(props.status.set)}</span>
        <Badge tone={props.status.offered ? "info" : "neutral"}>
          {props.status.offered ? t.set.offered : t.set.running}
        </Badge>
        <span class="text-fg-muted text-xs">
          {t.set.packages(props.status.kept, formatBytes(props.status.bytes))}
        </span>
        <span class="ml-auto">
          <Show when={failing() > 0} fallback={<Badge tone="ok">{t.set.healthy}</Badge>}>
            <Badge tone="crit">{t.set.failing(failing())}</Badge>
          </Show>
        </span>
      </summary>
      <div class="flex flex-col gap-2 border-edge border-t pb-2">
        <Table
          label={setLabel(props.status.set)}
          columns={columns}
          rows={props.status.feeds}
          rowKey={(feed) => feed.url}
        />
        <Show when={props.status.missing.length > 0}>
          <p class="px-3 text-warn text-xs">{t.set.missing(props.status.missing.join(", "))}</p>
        </Show>
        <Show when={props.status.failed.length > 0}>
          <div class="px-3 text-xs">
            <p class="font-medium text-crit">{t.set.failed}</p>
            <ul class="mt-1 flex flex-col gap-0.5 text-fg-muted">
              <For each={props.status.failed}>
                {(why) => <li class="break-words font-mono">{why}</li>}
              </For>
            </ul>
          </div>
        </Show>
      </div>
    </details>
  );
}
