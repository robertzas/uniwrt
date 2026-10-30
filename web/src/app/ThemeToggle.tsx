import { SegmentedControl } from "@kobalte/core/segmented-control";
import { Monitor, Moon, Sun } from "lucide";
import { For } from "solid-js";
import { isThemePreference, setThemePreference, themePreference } from "../lib/appearance";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";

const options = [
  { value: "system", label: strings.theme.system, icon: Monitor },
  { value: "light", label: strings.theme.light, icon: Sun },
  { value: "dark", label: strings.theme.dark, icon: Moon },
] as const;

/** System, light or dark: a radio group, so it works with arrow keys and screen readers. */
export function ThemeToggle() {
  return (
    <SegmentedControl
      value={themePreference()}
      onChange={(value) => {
        if (isThemePreference(value)) setThemePreference(value);
      }}
      class="flex items-center justify-between gap-3"
    >
      <SegmentedControl.Label class="text-fg-subtle text-xs">
        {strings.theme.label}
      </SegmentedControl.Label>
      <div role="presentation" class="relative flex rounded-lg border border-edge bg-canvas p-0.5">
        <SegmentedControl.Indicator class="absolute rounded-md bg-raised shadow-sm transition-transform duration-200" />
        <For each={options}>
          {(option) => (
            <SegmentedControl.Item value={option.value} class="relative">
              <SegmentedControl.ItemInput class="peer" />
              <SegmentedControl.ItemLabel
                title={option.label}
                class="flex size-7 cursor-pointer items-center justify-center rounded-md text-fg-subtle transition-colors hover:text-fg data-[checked]:text-fg peer-focus-visible:outline-2 peer-focus-visible:outline-accent"
              >
                <Icon icon={option.icon} class="size-3.5" />
                <span class="sr-only">{option.label}</span>
              </SegmentedControl.ItemLabel>
            </SegmentedControl.Item>
          )}
        </For>
      </div>
    </SegmentedControl>
  );
}
