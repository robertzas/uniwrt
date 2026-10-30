import * as KDialog from "@kobalte/core/dialog";
import type { JSX } from "@solidjs/web";
import { X } from "lucide";
import { Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "./Icon";

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  /** Keeps the title for screen readers only, for dialogs that show their own header. */
  hideTitle?: boolean;
  /** The element to focus on open; by default the first focusable one (the close button). */
  initialFocus?: () => HTMLElement | undefined;
  children: JSX.Element;
  class?: string;
}

const overlay =
  "fixed inset-0 z-40 bg-canvas/70 backdrop-blur-sm data-expanded:animate-fade-in data-closed:animate-fade-out";

/** A modal dialog: focus is trapped inside, and Escape or a click outside closes it. */
export function Dialog(props: DialogProps) {
  return (
    <KDialog.Root open={props.open} onOpenChange={props.onOpenChange}>
      <KDialog.Portal>
        <KDialog.Overlay class={overlay} />
        <div class="fixed inset-0 z-50 flex items-start justify-center px-4 pt-[12vh]">
          <KDialog.Content
            onOpenAutoFocus={(event) => focusInitial(event, props.initialFocus)}
            class={[
              "relative w-full max-w-lg rounded-card border border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out",
              props.class,
            ]}
          >
            <Header {...props} />
            {props.children}
          </KDialog.Content>
        </div>
      </KDialog.Portal>
    </KDialog.Root>
  );
}

export interface DrawerProps extends DialogProps {
  side?: "left" | "right";
}

/** A modal panel that slides in from a side of the screen. */
export function Drawer(props: DrawerProps) {
  const left = () => props.side === "left";
  return (
    <KDialog.Root open={props.open} onOpenChange={props.onOpenChange}>
      <KDialog.Portal>
        <KDialog.Overlay class={overlay} />
        <KDialog.Content
          onOpenAutoFocus={(event) => focusInitial(event, props.initialFocus)}
          class={[
            "fixed inset-y-0 z-50 flex w-80 max-w-[85vw] flex-col border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel",
            left()
              ? "left-0 border-r data-expanded:animate-slide-in-left data-closed:animate-slide-out-left"
              : "right-0 border-l data-expanded:animate-slide-in-right data-closed:animate-slide-out-right",
            props.class,
          ]}
        >
          <Header {...props} />
          {props.children}
        </KDialog.Content>
      </KDialog.Portal>
    </KDialog.Root>
  );
}

/**
 * Focuses `target` (else Kobalte focuses the first focusable element) unless the user already
 * focused something inside the dialog: the auto-focus can come a moment after the dialog shows.
 */
function focusInitial(event: Event, target: DialogProps["initialFocus"]) {
  const content = event.currentTarget instanceof Element ? event.currentTarget : null;
  const active = document.activeElement;
  if (content && active && active !== content && content.contains(active)) {
    event.preventDefault();
    return;
  }
  const element = target?.();
  if (element) {
    event.preventDefault();
    element.focus();
  }
}

function Header(props: DialogProps) {
  return (
    <>
      <KDialog.CloseButton
        aria-label={strings.common.close}
        class="absolute top-2.5 right-2.5 z-10 grid size-8 place-items-center rounded-control text-fg-muted hover:bg-raised hover:text-fg"
      >
        <Icon icon={X} class="size-4" />
      </KDialog.CloseButton>
      <Show
        when={!props.hideTitle}
        fallback={<KDialog.Title class="sr-only">{props.title}</KDialog.Title>}
      >
        <div class="px-5 pt-4 pr-12 pb-2">
          <KDialog.Title class="font-semibold text-base tracking-tight">
            {props.title}
          </KDialog.Title>
          <Show when={props.description}>
            {(description) => (
              <KDialog.Description class="mt-1 text-fg-muted text-sm">
                {description()}
              </KDialog.Description>
            )}
          </Show>
        </div>
      </Show>
    </>
  );
}
