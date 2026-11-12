import { TriangleAlert } from "lucide";
import { For } from "solid-js";
import { Icon } from "../../ui/Icon";

/** A warning naming the devices that run the basic wpad (no 802.11v), each linked to its Config tab. */
export function WpadNote(props: { text: string; nodes: readonly { id: number; name: string }[] }) {
  return (
    <div class="flex items-start gap-3 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
      <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
      <p class="min-w-0 flex-1">
        {props.text}{" "}
        <For each={props.nodes}>
          {(node, i) => (
            <>
              {i() > 0 ? ", " : ""}
              <a
                href={`/devices/${node.id}?tab=config`}
                class="font-medium text-accent underline-offset-2 hover:underline"
              >
                {node.name}
              </a>
            </>
          )}
        </For>
      </p>
    </div>
  );
}
