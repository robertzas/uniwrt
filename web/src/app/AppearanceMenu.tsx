import * as Popover from "@kobalte/core/popover";
import { Palette } from "lucide";
import { Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { SkinPicker } from "./SkinPicker";
import { ThemeToggle } from "./ThemeToggle";

/** The skin and theme choices, in a popover from the sidebar's footer. */
export function AppearanceMenu(props: { compact?: boolean }) {
  return (
    <Popover.Root placement={props.compact ? "right-end" : "top-start"} gutter={8}>
      <Popover.Trigger
        aria-label={props.compact ? strings.appearance.label : undefined}
        class={[
          "flex h-8 items-center gap-2.5 rounded-control text-fg-muted text-sm transition-colors hover:bg-raised hover:text-fg data-expanded:bg-raised data-expanded:text-fg",
          props.compact ? "w-full justify-center" : "w-full px-2.5",
        ]}
      >
        <Icon icon={Palette} class="size-4 shrink-0" />
        <Show when={!props.compact}>
          <span>{strings.appearance.label}</span>
        </Show>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 w-72 rounded-card border border-edge bg-surface p-3 shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
          <Popover.Title class="sr-only">{strings.appearance.label}</Popover.Title>
          <SkinPicker />
          <div class="mt-3 border-edge border-t pt-3">
            <ThemeToggle />
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
