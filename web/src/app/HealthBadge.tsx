import { useQuery } from "@tanstack/solid-query";
import { Loading, Match, Show, Switch } from "solid-js";
import { api } from "../lib/api";
import { strings } from "../lib/strings";

type State = "checking" | "online" | "offline";

const dot: Record<State, string> = {
  checking: "bg-fg-subtle animate-pulse",
  online: "bg-ok",
  offline: "bg-crit",
};

/**
 * Whether the controller answers, and which build it runs. Polls until live updates land.
 * Branches on the status flags: solid-query 6.0.0-rc.5 resolves the data of a failed first fetch
 * to undefined instead of throwing to an <Errored> boundary (D-045). Data is read only once the
 * query succeeded, and <Loading> keeps the badge visible while a poll revalidates it.
 */
export function HealthBadge() {
  const health = useQuery(() => ({
    queryKey: ["health"],
    queryFn: ({ signal }) => api.health(signal),
    refetchInterval: 15_000,
    retry: false,
  }));

  return (
    <Switch fallback={<Badge state="checking" />}>
      <Match when={health.isError}>
        <Badge state="offline" title={strings.health.unreachable} />
      </Match>
      <Match when={health.isSuccess}>
        <Loading fallback={<Badge state="checking" />}>
          <Badge
            state="online"
            version={health.data?.version}
            title={health.data && strings.health.details(health.data.commit, health.data.built_at)}
          />
        </Loading>
      </Match>
    </Switch>
  );
}

function Badge(props: { state: State; version?: string; title?: string }) {
  return (
    <div
      role="status"
      title={props.title}
      class="inline-flex h-7 items-center gap-2 rounded-full border border-edge bg-surface px-2.5 text-xs"
    >
      <span class={["size-2 rounded-full", dot[props.state]]} aria-hidden="true" />
      <span class="font-medium">{strings.health[props.state]}</span>
      <Show when={props.version}>
        {(version) => (
          <span class="font-mono text-fg-subtle tabular-nums">
            {strings.health.version(version())}
          </span>
        )}
      </Show>
    </div>
  );
}
