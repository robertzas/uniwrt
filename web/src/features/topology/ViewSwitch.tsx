import { SegmentedControl } from "@kobalte/core/segmented-control";
import { For } from "solid-js";
import { strings } from "../../lib/strings";
import { type ViewKind, viewKinds } from "./scene";

/** Tree, RF / mesh or clients: a radio group, so it works with arrow keys and screen readers. */
export function ViewSwitch(props: { value: ViewKind; onChange: (view: ViewKind) => void }) {
  return (
    <SegmentedControl
      value={props.value}
      onChange={(value) => {
        const view = viewKinds.find((kind) => kind === value);
        if (view) props.onChange(view);
      }}
      aria-label={strings.topology.views.label}
    >
      <div
        role="presentation"
        class="relative flex rounded-control border border-edge bg-canvas p-0.5"
      >
        <SegmentedControl.Indicator class="absolute rounded-[calc(var(--radius-control)-2px)] bg-raised shadow-sm transition-transform duration-200" />
        <For each={viewKinds}>
          {(kind) => (
            <SegmentedControl.Item value={kind} class="relative">
              <SegmentedControl.ItemInput class="peer" />
              <SegmentedControl.ItemLabel class="flex h-7 cursor-pointer items-center rounded-[calc(var(--radius-control)-2px)] px-3 font-medium text-fg-subtle text-xs transition-colors hover:text-fg data-[checked]:text-fg peer-focus-visible:outline-2 peer-focus-visible:outline-accent">
                {strings.topology.views[kind]}
              </SegmentedControl.ItemLabel>
            </SegmentedControl.Item>
          )}
        </For>
      </div>
    </SegmentedControl>
  );
}
