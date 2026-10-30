import * as RadioGroup from "@kobalte/core/radio-group";
import { Check, LayoutGrid, Sparkles, Square } from "lucide";
import { For } from "solid-js";
import { currentSkin, isSkin, setCurrentSkin } from "../lib/appearance";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";

const options = [
  { value: "graphite", icon: Square, ...strings.skin.graphite },
  { value: "glass", icon: Sparkles, ...strings.skin.glass },
  { value: "unifi", icon: LayoutGrid, ...strings.skin.unifi },
] as const;

/** The three skins (D-046) as a radio group: arrow keys move, and screen readers hear a choice. */
export function SkinPicker() {
  return (
    <RadioGroup.Root
      value={currentSkin()}
      onChange={(value) => {
        if (isSkin(value)) setCurrentSkin(value);
      }}
      class="flex flex-col gap-1.5"
    >
      <RadioGroup.Label class="mb-0.5 text-fg-subtle text-xs">
        {strings.skin.label}
      </RadioGroup.Label>
      <For each={options}>
        {(option) => (
          <RadioGroup.Item value={option.value} class="relative">
            <RadioGroup.ItemInput class="peer" />
            <RadioGroup.ItemLabel class="flex cursor-pointer items-center gap-3 rounded-control border border-edge px-3 py-2 transition-colors hover:bg-raised peer-focus-visible:outline-2 peer-focus-visible:outline-accent data-checked:border-accent data-checked:bg-raised">
              <Icon icon={option.icon} class="size-4 shrink-0 text-fg-muted" />
              <span class="min-w-0 flex-1">
                <span class="block font-medium text-sm">{option.label}</span>
                <span class="block truncate text-fg-subtle text-xs">{option.description}</span>
              </span>
              <RadioGroup.ItemIndicator class="text-accent">
                <Icon icon={Check} class="size-4" />
              </RadioGroup.ItemIndicator>
            </RadioGroup.ItemLabel>
          </RadioGroup.Item>
        )}
      </For>
    </RadioGroup.Root>
  );
}
