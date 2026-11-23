import type { JSX } from "@solidjs/web";
import type { UseQueryResult } from "@tanstack/solid-query";
import { RefreshCw, Search } from "lucide";
import { Show } from "solid-js";
import type { Client } from "../../../api/gen/Client";
import { errorText } from "../../../lib/api";
import { clientName } from "../../../lib/clients";
import { formatTime, NONE } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Icon } from "../../../ui/Icon";
import { Segmented } from "../../../ui/Segmented";
import type { FamilyFilter } from "./tables";

const t = strings.device.tools.tables;

const FAMILIES = [
  { value: "all", label: t.familyAll },
  { value: "ipv4", label: "IPv4" },
  { value: "ipv6", label: "IPv6" },
] as const;

/**
 * One of the device's tables, read on demand: the toolbar (the filter, the family, what the table
 * adds, how many rows show, when it was read, *Refresh*), then the table once read. A refresh keeps
 * the rows shown until the new ones arrive.
 */
export function TableView<D>(props: {
  query: UseQueryResult<D>;
  search: string;
  onSearch: (value: string) => void;
  family: FamilyFilter;
  onFamily: (value: FamilyFilter) => void;
  /** More filters, after the family. */
  filters?: JSX.Element;
  /** Rows shown and rows read, once read. */
  counts: { shown: number; all: number } | undefined;
  /** A line above the table, such as the kernel's count. */
  notice?: JSX.Element;
  children: (data: D) => JSX.Element;
}) {
  return (
    <div class="flex flex-col gap-3">
      <div class="flex flex-wrap items-center gap-2 px-4">
        <label class="relative">
          <span class="sr-only">{t.search}</span>
          <Icon
            icon={Search}
            class="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2.5 size-4 text-fg-subtle"
          />
          <input
            type="search"
            value={props.search}
            onInput={(event) => props.onSearch(event.currentTarget.value)}
            placeholder={t.searchPlaceholder}
            class="h-8 w-56 rounded-control border border-edge bg-canvas pr-3 pl-8 text-sm outline-none placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30"
          />
        </label>
        <Segmented
          label={t.family}
          value={props.family}
          options={FAMILIES}
          onChange={props.onFamily}
        />
        {props.filters}
        <span class="ml-auto text-fg-subtle text-xs tabular-nums">
          {props.counts ? t.shown(props.counts.shown, props.counts.all) : ""}
          <Show when={props.query.dataUpdatedAt > 0 && props.query.data}>
            {" · "}
            {t.readAt(formatTime(props.query.dataUpdatedAt))}
          </Show>
        </span>
        <Button
          size="sm"
          variant="secondary"
          icon={RefreshCw}
          disabled={props.query.isFetching}
          onClick={() => void props.query.refetch()}
        >
          {t.refresh}
        </Button>
      </div>
      <Show when={props.query.isError && props.query.error}>
        <p role="alert" class="px-4 text-crit text-sm">
          {errorText(props.query.error)}
        </p>
      </Show>
      {props.notice}
      <Show
        when={props.query.data}
        fallback={
          <Show when={!props.query.isError}>
            <p role="status" class="px-4 pb-4 text-fg-subtle text-sm">
              {t.reading}
            </p>
          </Show>
        }
      >
        {(data) => props.children(data())}
      </Show>
    </div>
  );
}

/** A client's name linking to its page, or a dash. */
export function ClientLink(props: { client: Client | undefined }) {
  return (
    <Show when={props.client} fallback={<span class="text-fg-subtle">{NONE}</span>}>
      {(client) => (
        <a
          href={`/clients/${encodeURIComponent(client().mac)}`}
          class="text-accent hover:underline focus-visible:outline-2 focus-visible:outline-accent"
        >
          {clientName(client())}
        </a>
      )}
    </Show>
  );
}

/** Monospaced text for addresses. */
export function Mono(props: { children: JSX.Element }) {
  return <span class="font-mono text-xs">{props.children}</span>;
}
