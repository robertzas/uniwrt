import { Eye } from "lucide";
import { Show } from "solid-js";
import type { NodeMode } from "../../api/gen/NodeMode";
import type { NodeState } from "../../api/gen/NodeState";
import type { NodeStatus } from "../../api/gen/NodeStatus";
import type { StatusReason } from "../../api/gen/StatusReason";
import { formatTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";

export const stateTone: Record<NodeState, Tone> = {
  online: "ok",
  degraded: "warn",
  offline: "crit",
};

/** A status reason in words, with when the controller tries again, if it says. */
export function describeReason(reason: StatusReason): string {
  const r = strings.devices.reason;
  const retry = (text: string, at: number) => `${text}. ${r.retryAt(formatTime(at))}`;
  switch (reason.kind) {
    case "connecting":
      return r.connecting(reason.attempt);
    case "unreachable":
      return retry(r.unreachable(reason.error), reason.retry_at);
    case "host_key_mismatch":
      return r.hostKeyMismatch;
    case "stopped":
      return r.stopped;
    case "no_agent":
      return r.noAgent;
    case "agent_starting":
      return r.agentStarting;
    case "agent_failed":
      return retry(r.agentFailed(reason.error), reason.retry_at);
    case "request_failed":
      return r.requestFailed(reason.op, reason.error);
    case "telemetry_stale":
      return r.telemetryStale;
    case "collect_failed":
      return r.collectFailed(reason.set, reason.error);
  }
}

/** The node's state as a badge; the most significant reason is its tooltip. */
export function StatusBadge(props: { status: NodeStatus }) {
  const reason = () => props.status.reasons[0];
  return (
    <span
      class="inline-flex"
      title={reason() ? describeReason(reason() as StatusReason) : undefined}
    >
      <Badge tone={stateTone[props.status.state]}>
        {strings.devices.state[props.status.state]}
      </Badge>
    </span>
  );
}

/** Observe mode is read-only (directive 8); managed nodes show nothing extra. */
export function ModeBadge(props: { mode: NodeMode }) {
  return (
    <Show when={props.mode === "observe"}>
      <span class="inline-flex" title={strings.devices.mode.observeHint}>
        <Badge tone="info" icon={Eye}>
          {strings.devices.mode.observe}
        </Badge>
      </span>
    </Show>
  );
}
