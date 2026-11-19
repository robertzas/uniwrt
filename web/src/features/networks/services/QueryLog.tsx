import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { RefreshCw } from "lucide";
import { createMemo, createSignal, createUniqueId, For, onCleanup, Show } from "solid-js";
import type { Blocking } from "../../../api/gen/Blocking";
import type { DnsClientStats } from "../../../api/gen/DnsClientStats";
import type { DnsOutcome } from "../../../api/gen/DnsOutcome";
import type { DnsQuery } from "../../../api/gen/DnsQuery";
import { api, type DnsQueryFilter } from "../../../lib/api";
import { formatCount, formatTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { FormField, INPUT, SELECT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import { type Column, Table } from "../../../ui/Table";
import { DomainAction, type Listing } from "./DomainAction";
import { OUTCOME_TONE } from "./dnsStats";

const t = strings.networks.queries;
const v = t.logView;
/** A page of queries, and the most the dialog holds. */
const PAGE = 200;
const MAX_ROWS = 5_000;
/** How long typing in the domain filter settles before it is read. */
const TYPING_MS = 300;
const OUTCOMES: readonly DnsOutcome[] = ["forwarded", "cached", "local", "blocked", "failed"];

type Row = DnsQuery & { n: number };

interface Older {
  key: string;
  queries: DnsQuery[];
  next: string | null;
  status: "idle" | "loading" | "failed";
}

/**
 * The stored queries, newest first, filtered by client, outcome and domain text; older pages on
 * demand (as on the logs page). The list is read when the filter changes or on Refresh, not live:
 * new queries would open gaps between pages.
 */
export function QueryLog(props: {
  initial: DnsQueryFilter;
  /** The clients of the statistics, for the client filter. */
  clients: readonly DnsClientStats[];
  /** A client's name by its key (MAC or address) and MAC. */
  nameOf: (key: string, mac: string | null) => string;
  blocking: Blocking;
  admin: boolean;
  listing: Listing;
  onClose: () => void;
}) {
  const id = createUniqueId();
  const [client, setClient] = createSignal(props.initial.client ?? "");
  const [outcome, setOutcome] = createSignal<DnsOutcome | "">(props.initial.outcome ?? "");
  const [typed, setTyped] = createSignal(props.initial.domain ?? "");
  const [domain, setDomain] = createSignal(props.initial.domain ?? "");
  let typing: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(typing));
  const type = (text: string) => {
    setTyped(text);
    clearTimeout(typing);
    typing = setTimeout(() => setDomain(text.trim()), TYPING_MS);
  };

  const filter = createMemo<DnsQueryFilter>(() => ({
    client: client() || undefined,
    outcome: outcome() || undefined,
    domain: domain() || undefined,
  }));
  const key = () => JSON.stringify(filter());
  const first = useQuery(() => ({
    queryKey: ["dns-queries", key()],
    queryFn: ({ signal }) => api.services.dnsQueries(filter(), null, PAGE, signal),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    placeholderData: keepPreviousData,
  }));
  const [older, setOlder] = createSignal<Older>();
  const current = () => {
    const now = older();
    return now?.key === key() && !first.isPlaceholderData ? now : undefined;
  };
  // Numbered: two queries can match in every field (a client asking again within the millisecond).
  const rows = createMemo<Row[]>(() =>
    [...(first.data?.queries ?? []), ...(current()?.queries ?? [])].map((q, n) => ({ ...q, n })),
  );
  const next = () => {
    if (first.isPlaceholderData) return null;
    const now = current();
    return now ? now.next : (first.data?.next ?? null);
  };

  const more = async () => {
    const k = key();
    const before = next();
    if (before === null || current()?.status === "loading") return;
    const base = current()?.queries ?? [];
    const query = filter();
    setOlder({ key: k, queries: base, next: before, status: "loading" });
    try {
      const page = await api.services.dnsQueries(query, before, PAGE);
      setOlder((now) =>
        now?.key === k && now.next === before
          ? { key: k, queries: [...now.queries, ...page.queries], next: page.next, status: "idle" }
          : now,
      );
    } catch {
      setOlder((now) => (now?.key === k ? { ...now, status: "failed" } : now));
    }
  };
  const refresh = () => {
    setOlder(undefined);
    void first.refetch();
  };

  const columns: readonly Column<Row>[] = [
    {
      id: "time",
      header: v.time,
      cell: (q) => <span class="whitespace-nowrap tabular-nums">{formatTime(q.at)}</span>,
    },
    {
      id: "client",
      header: v.client,
      cell: (q) => <span class="whitespace-nowrap">{props.nameOf(q.mac ?? q.client, q.mac)}</span>,
    },
    { id: "type", header: v.type, cell: (q) => <span class="text-fg-muted">{q.qtype}</span> },
    {
      id: "domain",
      header: v.domainColumn,
      cell: (q) => <span class="break-all">{q.domain}</span>,
    },
    {
      id: "outcome",
      header: v.outcome,
      cell: (q) => <Badge tone={OUTCOME_TONE[q.outcome]}>{t.outcomes[q.outcome]}</Badge>,
    },
    {
      id: "action",
      header: v.action,
      align: "end",
      cell: (q) => (
        <DomainAction
          domain={q.domain}
          blocked={q.outcome === "blocked"}
          blocking={props.blocking}
          admin={props.admin}
          listing={props.listing}
        />
      ),
    },
  ];

  return (
    <Dialog
      open
      size="wide"
      onOpenChange={(open) => !open && props.onClose()}
      title={v.title}
      description={v.description}
    >
      <div class="flex flex-col gap-3 px-5 pb-5">
        <div class="grid items-end gap-3 sm:grid-cols-[1fr_1fr_1.4fr_auto]">
          <FormField label={v.client} for={`${id}-client`}>
            <select
              id={`${id}-client`}
              class={SELECT}
              value={client()}
              onChange={(event) => setClient(event.currentTarget.value)}
            >
              <option value="">{v.anyClient}</option>
              <For each={props.clients}>
                {(c) => <option value={c.client}>{props.nameOf(c.client, c.mac)}</option>}
              </For>
            </select>
          </FormField>
          <FormField label={v.outcome} for={`${id}-outcome`}>
            <select
              id={`${id}-outcome`}
              class={SELECT}
              value={outcome()}
              onChange={(event) => setOutcome(event.currentTarget.value as DnsOutcome | "")}
            >
              <option value="">{v.anyOutcome}</option>
              <For each={OUTCOMES}>{(o) => <option value={o}>{t.outcomes[o]}</option>}</For>
            </select>
          </FormField>
          <FormField label={v.domain} for={`${id}-domain`}>
            <input
              id={`${id}-domain`}
              type="search"
              class={[INPUT, "border-edge"]}
              placeholder={v.domainPlaceholder}
              value={typed()}
              onInput={(event) => type(event.currentTarget.value)}
            />
          </FormField>
          <Button
            variant="secondary"
            icon={RefreshCw}
            onClick={refresh}
            disabled={first.isFetching}
          >
            {v.refresh}
          </Button>
        </div>
        <Show
          when={first.data}
          fallback={
            <p role="status" class="py-8 text-center text-fg-subtle text-sm">
              {first.isError ? "" : v.loading}
            </p>
          }
        >
          <Table
            label={v.table}
            columns={columns}
            rows={rows()}
            rowKey={(q) => `${q.n}:${q.at}:${q.client}:${q.qtype}:${q.domain}`}
            empty={v.empty}
            class="max-h-[60vh] rounded-control border border-edge"
          />
          <Footer
            status={current()?.status ?? "idle"}
            hasOlder={next() !== null && rows().length < MAX_ROWS}
            capped={rows().length >= MAX_ROWS}
            onMore={() => void more()}
          />
        </Show>
        <RequestError text={first.error?.message ?? props.listing.failure()} />
        <div class="flex justify-end">
          <Button variant="secondary" onClick={() => props.onClose()}>
            {v.close}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

function Footer(props: {
  status: Older["status"];
  hasOlder: boolean;
  capped: boolean;
  onMore: () => void;
}) {
  return (
    <div class="flex justify-center text-fg-subtle text-xs">
      <Show
        when={props.status !== "loading"}
        fallback={<span role="status">{v.loadingOlder}</span>}
      >
        <Show
          when={props.hasOlder}
          fallback={<span>{props.capped ? v.capped(formatCount(MAX_ROWS)) : v.end}</span>}
        >
          <div class="flex items-center gap-2">
            <Show when={props.status === "failed"}>
              <span role="alert">{v.olderFailed}</span>
            </Show>
            <Button size="sm" variant="ghost" onClick={() => props.onMore()}>
              {v.loadOlder}
            </Button>
          </div>
        </Show>
      </Show>
    </div>
  );
}
