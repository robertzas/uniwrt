import { useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import { SessionContext } from "../../../lib/session";
import { ManagementCard } from "./ManagementCard";
import { PackagesCard } from "./PackagesCard";

/** A device's Config tab (ui.md, Device page): its management and the packages uniwrt owns. */
export function ConfigTab(props: { node: Node }) {
  const session = useContext(SessionContext);
  const admin = () => session.user.role === "admin";
  return (
    <div class="grid items-start gap-4 lg:grid-cols-5">
      <div class="min-w-0 lg:col-span-2">
        <ManagementCard node={props.node} admin={admin()} />
      </div>
      <div class="min-w-0 lg:col-span-3">
        <PackagesCard node={props.node} admin={admin()} />
      </div>
    </div>
  );
}
