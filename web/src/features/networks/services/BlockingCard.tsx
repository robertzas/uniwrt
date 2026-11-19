import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { Info, RefreshCw, TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { BlocklistsView } from "../../../api/gen/BlocklistsView";
import type { Client } from "../../../api/gen/Client";
import type { SiteView } from "../../../api/gen/SiteView";
import { api, errorText } from "../../../lib/api";
import { formatCount, formatDateTime, NONE } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { BlockingEditor } from "./BlockingEditor";
import { isBusy, listState, syncStatus } from "./blocking";

const t = strings.networks.blocking;
/** How often the state is read while lists download or go out, and otherwise. */
const BUSY_POLL_MS = 3_000;
const IDLE_POLL_MS = 30_000;

/**
 * DNS filtering (network.md, Blocklists; D-133): whether the gateway holds the compiled lists,
 * each list's download state and the compile's size against the gateway's capacity. Admins
 * download the lists again on demand and edit the settings in a dialog.
 */
export function BlockingCard(props: {
  view: SiteView;
  admin: boolean;
  /** The known clients, to name and pick the bypassing ones. */
  clients: readonly Client[];
}) {
  const queryClient = useQueryClient();
  const state = useQuery(() => ({
    queryKey: ["blocklists"],
    queryFn: ({ signal }) => api.services.blocklists(signal),
    refetchInterval: (query: { state: { data: BlocklistsView | undefined } }) =>
      isBusy(query.state.data) ? BUSY_POLL_MS : IDLE_POLL_MS,
  }));
  const [editing, setEditing] = createSignal<{ site: SiteView; lists: BlocklistsView }>();
  const [refreshing, setRefreshing] = createSignal(false);
  const [failure, setFailure] = createSignal<string>();
  const saved = () => props.view.config.services.blocking;
  /** On in the latest revision, or still on the gateway. */
  const on = () => saved().enabled || state.data?.enabled === true;

  const refresh = async () => {
    setRefreshing(true);
    setFailure(undefined);
    try {
      queryClient.setQueryData(["blocklists"], await api.services.refreshBlocklists());
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && state.data}>
            {(lists) => (
              <div class="flex gap-2">
                <Show when={on()}>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={RefreshCw}
                    disabled={refreshing() || lists().refreshing}
                    onClick={() => void refresh()}
                  >
                    {t.refresh}
                  </Button>
                </Show>
                <Button size="sm" onClick={() => setEditing({ site: props.view, lists: lists() })}>
                  {t.edit}
                </Button>
              </div>
            )}
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={state.data}
          fallback={
            <p role="status" class="text-fg-muted text-sm">
              {state.isError ? "" : t.loading}
            </p>
          }
        >
          {(lists) => (
            <Show when={on()} fallback={<Off />}>
              <Summary lists={lists()} view={props.view} />
            </Show>
          )}
        </Show>
        <RequestError text={failure() ?? state.error?.message} />
      </div>
      <Show when={editing()}>
        {(open) => (
          <BlockingEditor
            view={open().site}
            lists={open().lists}
            clients={props.clients}
            onClose={() => setEditing(undefined)}
            onSaved={() => void queryClient.invalidateQueries({ queryKey: ["blocklists"] })}
          />
        )}
      </Show>
    </Card>
  );
}

function Off() {
  return (
    <div class="text-sm">
      <p>{t.off}</p>
      <p class="text-fg-muted text-xs">{t.offHint}</p>
    </div>
  );
}

function Summary(props: { lists: BlocklistsView; view: SiteView }) {
  const saved = () => props.view.config.services.blocking;
  const status = () => syncStatus(props.lists);
  const pending = () => {
    if (saved().enabled && !props.lists.enabled) return t.pendingApply;
    if (!saved().enabled && props.lists.enabled) return t.pendingOff;
    return undefined;
  };
  const cut = () => {
    const compiled = props.lists.compiled;
    return compiled && compiled.cut > 0
      ? t.cut(formatCount(compiled.cut), formatCount(compiled.capacity))
      : undefined;
  };
  const checked = () =>
    props.lists.refreshing
      ? t.refreshing
      : props.lists.refreshed_at === null
        ? undefined
        : t.refreshed(formatDateTime(props.lists.refreshed_at));
  const figures = () => [
    {
      label: t.stats.blocked,
      value: props.lists.compiled ? formatCount(props.lists.compiled.domains) : NONE,
    },
    { label: t.stats.lists, value: formatCount(props.lists.lists.length) },
    { label: t.stats.allowed, value: formatCount(saved().allow.length) },
    { label: t.stats.bypassed, value: formatCount(saved().bypass.length) },
  ];

  return (
    <>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
        <p class="flex min-w-0 flex-1 items-center gap-2" role="status">
          <StatusDot tone={status().tone} pulse={status().pulse} />
          <span>{status().text}</span>
        </p>
        <Show when={checked()}>
          {(text) => <span class="text-fg-subtle text-xs">{text()}</span>}
        </Show>
      </div>
      <Show when={pending()}>
        {(text) => (
          <p class="flex items-center gap-2 rounded-control border border-info/40 bg-info/10 px-3 py-2 text-sm">
            <Icon icon={Info} class="size-4 shrink-0 text-info" />
            {text()}
          </p>
        )}
      </Show>
      <Show when={cut()}>
        {(text) => (
          <p class="flex items-center gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-sm">
            <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
            {text()}
          </p>
        )}
      </Show>
      <dl class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <For each={figures()}>
          {(figure) => (
            <div class="rounded-control border border-edge px-3 py-2">
              <dt class="text-fg-subtle text-xs">{figure.label}</dt>
              <dd class="font-semibold text-lg tabular-nums">{figure.value}</dd>
            </div>
          )}
        </For>
      </dl>
      <Show when={props.lists.lists.length > 0}>
        <ul
          class="divide-y divide-edge rounded-control border border-edge"
          aria-label={t.stats.lists}
        >
          <For each={props.lists.lists}>
            {(list) => {
              const state = () => listState(list);
              return (
                <li
                  class="flex flex-wrap items-center gap-x-3 gap-y-0.5 px-3 py-2"
                  aria-label={list.name}
                >
                  <span class="min-w-0 flex-1 truncate text-sm">
                    {list.name}
                    <Show when={list.id === null}>
                      <span class="ml-2 text-fg-subtle text-xs">{t.list.custom}</span>
                    </Show>
                  </span>
                  <span class="flex items-center gap-2 text-fg-muted text-xs">
                    <StatusDot tone={state().tone} />
                    {state().text}
                  </span>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
    </>
  );
}
