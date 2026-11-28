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
  /** `wide` fits lists and tables (64 rem), `lg` two-column forms (42 rem); `md` (32 rem) by default. */
  size?: "md" | "lg" | "wide";
  class?: string;
}

const overlay =
  "fixed inset-0 z-40 bg-canvas/70 backdrop-blur-sm data-expanded:animate-fade-in data-closed:animate-fade-out";

/**
 * A modal dialog: focus is trapped inside, and Escape or a click outside closes it. Below `md` it
 * is a sheet from the bottom edge (D-148). A body taller than the screen scrolls inside the dialog
 * (the scroll lock lets the wheel and touch scroll only the dialog's own content, D-157): its title
 * stays on top, and a `DialogFooter` keeps the actions at the bottom, in reach of a thumb.
 */
export function Dialog(props: DialogProps) {
  const focus = createFocusReturn();
  return (
    <KDialog.Root open={props.open} onOpenChange={props.onOpenChange}>
      <KDialog.Portal>
        <KDialog.Overlay class={overlay} />
        <div class="fixed inset-0 z-50 flex items-start justify-center overflow-hidden px-4 pt-[12vh] pb-[6vh] max-md:items-end max-md:px-0 max-md:pt-3 max-md:pb-0">
          <KDialog.Content
            onOpenAutoFocus={(event) => focus.open(event, props.initialFocus)}
            onCloseAutoFocus={focus.close}
            class={[
              "relative max-h-full w-full overflow-y-auto overscroll-contain rounded-card border border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out",
              { wide: "max-w-5xl", lg: "max-w-2xl", md: "max-w-lg" }[props.size ?? "md"],
              // Below md, a sheet from the bottom edge as wide as the screen.
              "max-md:max-w-none max-md:rounded-b-none max-md:border-x-0 max-md:border-b-0 max-md:pb-[env(safe-area-inset-bottom)] max-md:data-expanded:animate-sheet-in max-md:data-closed:animate-sheet-out",
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
  const focus = createFocusReturn();
  return (
    <KDialog.Root open={props.open} onOpenChange={props.onOpenChange}>
      <KDialog.Portal>
        <KDialog.Overlay class={overlay} />
        <KDialog.Content
          onOpenAutoFocus={(event) => focus.open(event, props.initialFocus)}
          onCloseAutoFocus={focus.close}
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
 * Kobalte returns focus to its `Trigger` on close, but these dialogs open from state, so there is
 * none: this remembers what had focus when the dialog opened and returns focus there, if it is
 * still in the page (an opener that unmounted, such as the ⌘K palette, leaves focus to the body).
 */
function createFocusReturn() {
  let opener: HTMLElement | undefined;
  return {
    open(event: Event, target: DialogProps["initialFocus"]) {
      const active = document.activeElement;
      const content = event.currentTarget instanceof Element ? event.currentTarget : null;
      if (active instanceof HTMLElement && active !== document.body && !content?.contains(active)) {
        opener = active;
      }
      focusInitial(event, target);
    },
    close() {
      if (opener?.isConnected) opener.focus({ preventScroll: true });
      opener = undefined;
    },
  };
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

/** The close button and the title, which stay on top of a scrolling body. */
function Header(props: DialogProps) {
  return (
    <>
      <div class="sticky top-0 z-10 bg-surface glass:backdrop-blur-panel">
        <KDialog.CloseButton
          aria-label={strings.common.close}
          class="absolute top-2.5 right-2.5 grid size-8 place-items-center rounded-control text-fg-muted hover:bg-raised hover:text-fg"
        >
          <Icon icon={X} class="size-4" />
        </KDialog.CloseButton>
        <Show
          when={!props.hideTitle}
          fallback={<KDialog.Title class="sr-only">{props.title}</KDialog.Title>}
        >
          <div class={["px-5 pt-4 pr-12", props.description ? "pb-1" : "pb-2"]}>
            <KDialog.Title class="font-semibold text-base tracking-tight">
              {props.title}
            </KDialog.Title>
          </div>
        </Show>
      </div>
      <Show when={!props.hideTitle && props.description}>
        {(description) => (
          <KDialog.Description class="px-5 pr-12 pb-2 text-fg-muted text-sm">
            {description()}
          </KDialog.Description>
        )}
      </Show>
    </>
  );
}

/**
 * A dialog's actions, at the end of its body (`gap-2` unless `class` names a gap). They stick to
 * the dialog's bottom edge over a scrolling body, on the surface's color as far as a body's `px-5`
 * padding reaches.
 */
export function DialogFooter(props: { class?: string; children: JSX.Element }) {
  return (
    <div
      class={[
        "flex flex-wrap items-center justify-end",
        !props.class?.match(/(^|\s)gap-/) && "gap-2",
        "sticky bottom-0 z-10 bg-surface py-3 glass:backdrop-blur-panel shadow-[0_0_0_1.25rem_var(--color-surface)] [clip-path:inset(0_-1.25rem_-1.25rem)]",
        props.class,
      ]}
    >
      {props.children}
    </div>
  );
}
