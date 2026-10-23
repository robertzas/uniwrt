import { useQuery } from "@tanstack/solid-query";
import { Show } from "solid-js";
import { api } from "../lib/api";
import { strings } from "../lib/strings";

type State = "checking" | "online" | "offline";

const dot: Record<State, string> = {
  checking: "bg-fg-subtle animate-pulse",
  online: "bg-ok",
  offline: "bg-crit",
};

/** Whether the controller answers, and which build it runs. Polls until live updates land. */
export function HealthBadge() {
  const health = useQuery(() => ({
    queryKey: ["health"],
    queryFn: ({ signal }) => api.health(signal),
    refetchInterval: 15_000,
    retry: false,
  }));
  const state = (): State => {
    if (health.isError) return "offline";
    return health.data ? "online" : "checking";
  };
  const title = () => {
    if (health.isError) return strings.health.unreachable;
    const data = health.data;
    return data ? strings.health.details(data.commit, data.built_at) : undefined;
  };

  return (
    <div
      role="status"
      title={title()}
      class="inline-flex h-7 items-center gap-2 rounded-full border border-edge bg-surface px-2.5 text-xs"
    >
      <span class={`size-2 rounded-full ${dot[state()]}`} aria-hidden="true" />
      <span class="font-medium">{strings.health[state()]}</span>
      <Show when={state() === "online" && health.data}>
        {(data) => (
          <span class="font-mono text-fg-subtle tabular-nums">
            {strings.health.version(data().version)}
          </span>
        )}
      </Show>
    </div>
  );
}
