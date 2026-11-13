import type { JSX } from "@solidjs/web";
import { createMemo, For, Show } from "solid-js";
import type { Floor } from "../../api/gen/Floor";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { StatusDot, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { showToast } from "../../ui/Toast";
import type { Mode } from "./FloorEditor";
import { groupDevices } from "./floors";

const t = strings.floors.devices;

const tones: Record<NodeSummary["status"]["state"], Tone> = {
  online: "ok",
  degraded: "warn",
  offline: "crit",
};

/**
 * The fleet's devices beside the plan: those on this floor (admins take them off), on other
 * floors and not placed (admins place them here: the next click on the plan says where).
 */
export function DevicesPanel(props: {
  floor: Floor;
  floors: readonly Floor[];
  nodes: readonly NodeSummary[];
  admin: boolean;
  mode: Mode;
  onMode: (mode: Mode) => void;
  onRemoved: (node: number) => void;
}) {
  const groups = createMemo(() => groupDevices(props.nodes, props.floors, props.floor));
  const canPlace = () => props.admin && props.floor.image !== null;
  const placing = (node: number) => props.mode.kind === "place" && props.mode.node === node;
  const takeOff = async (node: number) => {
    try {
      await api.floors.unplace(props.floor.id, node);
      props.onRemoved(node);
    } catch (err) {
      showToast({ title: errorText(err) ?? strings.floors.dialog.failed, tone: "crit" });
    }
  };
  const place = (node: NodeSummary) =>
    props.onMode(
      placing(node.id) ? { kind: "idle" } : { kind: "place", node: node.id, name: node.name },
    );

  const row = (node: NodeSummary, detail: string | undefined, action: "place" | "off") => (
    <li class="flex min-h-10 items-center gap-2.5 py-1">
      <StatusDot tone={tones[node.status.state]} />
      <div class="min-w-0 flex-1">
        <p class="truncate font-medium text-sm">{node.name}</p>
        <Show when={detail}>
          <p class="truncate text-fg-subtle text-xs">{detail}</p>
        </Show>
      </div>
      <Show when={action === "off" && props.admin}>
        <Button size="sm" variant="ghost" onClick={() => void takeOff(node.id)}>
          {t.takeOff}
        </Button>
      </Show>
      <Show when={action === "place" && canPlace()}>
        <Button
          size="sm"
          variant={placing(node.id) ? "primary" : "secondary"}
          aria-pressed={placing(node.id) ? "true" : "false"}
          aria-label={`${placing(node.id) ? t.cancel : t.place} ${node.name}`}
          onClick={() => place(node)}
        >
          {placing(node.id) ? t.cancel : t.place}
        </Button>
      </Show>
    </li>
  );

  return (
    <Card aria-label={t.title} class="self-start">
      <CardHeader title={t.title} description={t.description} />
      <div class="flex flex-col gap-4 px-4 pb-4">
        <Show
          when={props.nodes.length > 0}
          fallback={<p class="text-fg-muted text-sm">{t.none}</p>}
        >
          <Show when={props.admin && props.floor.image === null}>
            <p class="text-fg-muted text-sm">{t.needsPlan}</p>
          </Show>
          <Group title={t.here} count={groups().here.length}>
            <For each={groups().here}>{(node) => row(node, undefined, "off")}</For>
          </Group>
          <Group title={t.unplaced} count={groups().unplaced.length}>
            <For each={groups().unplaced}>{(node) => row(node, undefined, "place")}</For>
          </Group>
          <Group title={t.elsewhere} count={groups().elsewhere.length}>
            <For each={groups().elsewhere}>
              {(entry) => row(entry.node, t.onFloor(entry.floor.name), "place")}
            </For>
          </Group>
        </Show>
      </div>
    </Card>
  );
}

function Group(props: { title: string; count: number; children: JSX.Element }) {
  return (
    <Show when={props.count > 0}>
      <section aria-label={props.title} class="flex flex-col">
        <h3 class="font-medium text-fg-subtle text-xs uppercase tracking-wide">{props.title}</h3>
        <ul class="divide-y divide-edge">{props.children}</ul>
      </section>
    </Show>
  );
}
