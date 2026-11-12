import { Radio, Router, Waypoints } from "lucide";
import { createSignal, Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeRole } from "../../../api/gen/NodeRole";
import { api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { RadioCards } from "../../../ui/RadioCards";
import { showToast } from "../../../ui/Toast";

const r = strings.device.role;
const ROLES: readonly NodeRole[] = ["gateway", "ap", "bridge"];
const ICONS = { gateway: Router, ap: Radio, bridge: Waypoints } as const;

/**
 * The device's role (adoption.md, Roles; D-100): what it is in the site, and for admins the
 * change, such as a router that becomes a dumb access point. The stored role changes at once;
 * the device itself only with the next apply, which the review shows first. A site keeps one
 * gateway, and a managed gateway keeps its role.
 */
export function RoleCard(props: { node: Node; admin: boolean }) {
  const live = useContext(LiveContext);
  const [asking, setAsking] = createSignal(false);
  // Set from the node's role each time the dialog opens.
  const [chosen, setChosen] = createSignal<NodeRole>("ap");
  const [sending, setSending] = createSignal(false);

  /** Why the device can't take the chosen role, if it can't. */
  const blocked = () => {
    const role = chosen();
    if (role === props.node.role) return undefined;
    if (role === "gateway") {
      const other = live.fleet.nodes.find((n) => n.role === "gateway" && n.id !== props.node.id);
      if (other) return r.otherGateway(other.name);
    }
    if (props.node.role === "gateway" && props.node.mode === "managed") return r.managedGateway;
    return undefined;
  };

  const open = () => {
    setChosen(props.node.role);
    setAsking(true);
  };

  const save = async () => {
    setSending(true);
    try {
      const { role } = await api.nodes.role(props.node.id, chosen());
      setAsking(false);
      showToast({
        title: r.done(props.node.name, strings.devices.role[role]),
        description: r.doneHint,
        tone: "ok",
      });
    } catch (err) {
      showToast({ title: r.failed, description: errorText(err), tone: "crit" });
    } finally {
      setSending(false);
    }
  };

  return (
    <Card>
      <CardHeader
        title={r.title}
        actions={
          <Show when={props.admin}>
            <Button variant="secondary" size="sm" onClick={open}>
              {r.change}
            </Button>
          </Show>
        }
      />
      <div class="flex items-start gap-3 px-4 pb-4">
        <Icon icon={ICONS[props.node.role]} class="mt-0.5 size-5 shrink-0 text-accent" />
        <div class="min-w-0 text-sm">
          <p class="font-medium">{strings.devices.role[props.node.role]}</p>
          <p class="mt-0.5 text-fg-muted">{r.what[props.node.role]}</p>
          <Show when={!props.admin}>
            <p class="mt-1.5 text-fg-subtle">{r.adminOnly}</p>
          </Show>
        </div>
      </div>
      <Dialog open={asking()} onOpenChange={setAsking} title={r.dialogTitle(props.node.name)}>
        <div class="flex flex-col gap-3 px-5 pt-2 pb-5">
          <RadioCards
            label={r.label}
            value={chosen()}
            class="sm:grid-cols-1"
            options={ROLES.map((role) => ({
              value: role,
              label: strings.devices.role[role],
              description: r.what[role],
            }))}
            onChange={setChosen}
          />
          <Show when={chosen() !== props.node.role}>
            <p
              class={["text-sm", blocked() ? "text-crit" : "text-fg-muted"]}
              role={blocked() ? "alert" : undefined}
            >
              {blocked() ?? r.becomes[chosen()]}
            </p>
          </Show>
          <div class="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setAsking(false)}>
              {r.cancel}
            </Button>
            <Button
              variant="primary"
              disabled={sending() || chosen() === props.node.role || blocked() !== undefined}
              onClick={save}
            >
              {r.save}
            </Button>
          </div>
        </div>
      </Dialog>
    </Card>
  );
}
