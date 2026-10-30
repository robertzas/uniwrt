import * as KToast from "@kobalte/core/toast";
import { X } from "lucide";
import { Show } from "solid-js";
import { strings } from "../lib/strings";
import { StatusDot, type Tone } from "./Badge";
import { Icon } from "./Icon";

export interface ToastOptions {
  title: string;
  description?: string;
  tone?: Tone;
  /** Milliseconds before it closes; it stays while hovered or focused. */
  duration?: number;
}

/** Shows a toast in the region that <Toaster> renders; returns its id. */
export function showToast(options: ToastOptions): number {
  return KToast.toaster.show((props) => (
    <KToast.Root
      toastId={props.toastId}
      duration={options.duration ?? 5000}
      class="flex items-start gap-3 rounded-card border border-edge bg-surface p-3.5 shadow-overlay glass:backdrop-blur-panel data-closed:animate-fade-out data-opened:animate-pop-in"
    >
      <StatusDot tone={options.tone ?? "info"} class="mt-1.5" />
      <div class="min-w-0 flex-1">
        <KToast.Title class="font-medium text-sm">{options.title}</KToast.Title>
        <Show when={options.description}>
          {(description) => (
            <KToast.Description class="mt-0.5 text-fg-muted text-sm">
              {description()}
            </KToast.Description>
          )}
        </Show>
      </div>
      <KToast.CloseButton
        aria-label={strings.common.dismiss}
        class="-m-1 grid size-7 shrink-0 place-items-center rounded-control text-fg-subtle hover:bg-raised hover:text-fg"
      >
        <Icon icon={X} class="size-3.5" />
      </KToast.CloseButton>
    </KToast.Root>
  ));
}

/** The live region toasts appear in; render once, in the app shell. */
export function Toaster() {
  return (
    <KToast.Region aria-label={strings.common.notifications}>
      <KToast.List class="fixed right-4 bottom-4 z-50 flex w-[min(24rem,calc(100vw-2rem))] flex-col gap-2 outline-none" />
    </KToast.Region>
  );
}
