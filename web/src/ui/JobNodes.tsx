import {
  Circle,
  CircleCheck,
  CircleCheckBig,
  CircleDot,
  CircleMinus,
  CircleX,
  LoaderCircle,
  Undo2,
} from "lucide";
import { For, Show } from "solid-js";
import { Icon, type IconNode } from "./Icon";

/** Where a node of a job on several stands (structurally the API's `JobNodeState`). */
export type NodeState =
  | "pending"
  | "unchanged"
  | "skipped"
  | "staged"
  | "applied"
  | "verified"
  | "confirmed"
  | "rolled_back"
  | "backed_up"
  | "failed";

/** One node of a job on several (structurally the API's `JobNode`; ui/ imports no API types). */
export interface NodesJobEntry {
  node: number;
  name: string;
  state: NodeState;
  detail: string | null;
}

const NODE_ICONS: Record<NodeState, { icon: IconNode; class: string }> = {
  pending: { icon: Circle, class: "text-fg-subtle" },
  unchanged: { icon: CircleMinus, class: "text-fg-subtle" },
  skipped: { icon: CircleMinus, class: "text-fg-subtle" },
  staged: { icon: CircleDot, class: "text-info" },
  applied: { icon: LoaderCircle, class: "animate-spin text-accent" },
  verified: { icon: CircleCheck, class: "text-accent" },
  confirmed: { icon: CircleCheckBig, class: "text-ok" },
  rolled_back: { icon: Undo2, class: "text-warn" },
  backed_up: { icon: CircleCheckBig, class: "text-ok" },
  failed: { icon: CircleX, class: "text-crit" },
};

/**
 * The nodes of a job on several, in its order: each one's state in the job's own words (`states`)
 * and what happened to it last, unless that only repeats the state. Spinners stop once it ends.
 */
export function JobNodes(props: {
  nodes: readonly NodesJobEntry[];
  label: string;
  states: Record<NodeState, string>;
  running: boolean;
  /** Overrides an icon's color, for a job whose last good state isn't `confirmed`. */
  doneState?: NodeState;
}) {
  const look = (state: NodeState) =>
    state === props.doneState ? NODE_ICONS.confirmed : NODE_ICONS[state];
  return (
    <div class="rounded-control border border-edge">
      <h3 class="border-edge/60 border-b px-3 py-2 font-medium text-fg-muted text-xs">
        {props.label}
      </h3>
      <ul aria-label={props.label} class="flex flex-col">
        <For each={props.nodes}>
          {(node) => (
            <li class="flex items-start gap-2.5 border-edge/40 border-b px-3 py-2 last:border-b-0">
              <Icon
                icon={look(node.state).icon}
                class={[
                  "mt-0.5 size-4 shrink-0",
                  look(node.state).class,
                  { "animate-none": !props.running },
                ]}
              />
              <div class="min-w-0 flex-1">
                <p class="flex flex-wrap items-baseline justify-between gap-x-2 text-sm">
                  <span class="font-medium text-fg">{node.name}</span>
                  <span class="text-fg-muted text-xs">{props.states[node.state]}</span>
                </p>
                <Show
                  when={
                    node.detail?.toLowerCase() !== props.states[node.state].toLowerCase() &&
                    node.detail
                  }
                >
                  {(detail) => <p class="mt-0.5 break-words text-fg-subtle text-xs">{detail()}</p>}
                </Show>
              </div>
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}
