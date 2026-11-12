import { Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { DriftCard, hasDrift } from "./DriftCard";
import { ManagementCard } from "./ManagementCard";
import { PackagesCard } from "./PackagesCard";
import { RootPasswordCard } from "./RootPasswordCard";
import { WpadCard } from "./WpadCard";

/**
 * A device's Config tab (ui.md, Device page): its management, its Wi-Fi daemon (the full wpad or
 * the basic one), for admins the root password the vault keeps, the packages uniwrt owns and,
 * once managed, their drift, which comes first while there is any.
 */
export function ConfigTab(props: { node: Node; facts: NodeFacts | null }) {
  const session = useContext(SessionContext);
  const live = useContext(LiveContext);
  const admin = () => session.user.role === "admin";
  return (
    <div class="grid items-start gap-4 lg:grid-cols-5">
      <div class="flex min-w-0 flex-col gap-4 lg:col-span-2">
        <ManagementCard node={props.node} admin={admin()} />
        <WpadCard node={props.node} facts={props.facts} admin={admin()} />
        <Show when={admin()}>
          <RootPasswordCard node={props.node} />
        </Show>
      </div>
      <div class="min-w-0 lg:col-span-3">
        <PackagesCard node={props.node} admin={admin()} />
      </div>
      <Show when={props.node.mode === "managed"}>
        <div
          class={["min-w-0 lg:col-span-5", { "order-first": hasDrift(live.config, props.node) }]}
        >
          <DriftCard node={props.node} admin={admin()} />
        </div>
      </Show>
    </div>
  );
}
