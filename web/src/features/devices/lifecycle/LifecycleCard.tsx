import { HardDriveDownload, PackageX, RotateCcw, Unlink } from "lucide";
import { createSignal, For, Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import type { IconNode } from "../../../ui/Icon";
import { type Removal, RemoveDialog } from "./RemoveDialog";
import { ReplaceDialog } from "./ReplaceDialog";

const l = strings.device.lifecycle;

type Action = "replace" | Removal;

const actions: { action: Action; icon: IconNode }[] = [
  { action: "replace", icon: HardDriveDownload },
  { action: "reset", icon: RotateCcw },
  { action: "decommission", icon: PackageX },
  { action: "forget", icon: Unlink },
];

/**
 * A device's lifecycle actions for admins (lifecycle.md, Replace / RMA / decommission; D-131):
 * replace its device, reset it to its defaults to be adopted back, decommission it or forget it.
 * The gateway can only be replaced while other devices depend on it; the server's other
 * refusals (a rollout still to upgrade it, a job running for it) show in the dialogs.
 */
export function LifecycleCard(props: { node: Node }) {
  const live = useContext(LiveContext);
  const [open, setOpen] = createSignal<Action>();
  const held = () => props.node.role === "gateway" && live.fleet.nodes.length > 1;
  const removal = () => {
    const action = open();
    return action === "replace" ? undefined : action;
  };
  return (
    <Card aria-label={l.title}>
      <CardHeader title={l.title} description={l.hint} />
      <ul class="flex flex-col divide-y divide-edge px-4 pb-2">
        <For each={actions}>
          {(entry) => {
            const t = l[entry.action];
            const disabled = () => entry.action !== "replace" && held();
            return (
              <li class="flex flex-wrap items-center gap-x-4 gap-y-2 py-3">
                <div class="min-w-0 flex-1 basis-60">
                  <p class="font-medium text-sm">{t.label}</p>
                  <p class="mt-0.5 text-fg-muted text-xs">{t.hint}</p>
                </div>
                <Button
                  size="sm"
                  variant="secondary"
                  icon={entry.icon}
                  disabled={disabled()}
                  onClick={() => setOpen(entry.action)}
                >
                  {t.action}
                </Button>
              </li>
            );
          }}
        </For>
      </ul>
      <Show when={held()}>
        <p class="px-4 pb-4 text-fg-subtle text-xs">{l.gatewayHeld}</p>
      </Show>
      <ReplaceDialog
        node={props.node}
        open={open() === "replace"}
        onClose={() => setOpen(undefined)}
      />
      <RemoveDialog node={props.node} action={removal()} onClose={() => setOpen(undefined)} />
    </Card>
  );
}
