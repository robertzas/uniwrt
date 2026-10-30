import { Dialog } from "@kobalte/core/dialog";
import { useLocation } from "@solidjs/router";
import type { JSX } from "@solidjs/web";
import { X } from "lucide";
import { createEffect, createSignal } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";

/** The app frame: a sidebar rail on wide screens, a drawer on phones, and the top bar. */
export function Shell(props: { children?: JSX.Element }) {
  const [menuOpen, setMenuOpen] = createSignal(false);
  const location = useLocation();
  createEffect(
    () => location.pathname,
    () => {
      setMenuOpen(false);
    },
    { defer: true },
  );

  return (
    <div class="flex min-h-dvh">
      <aside class="sticky top-0 hidden h-dvh w-60 shrink-0 border-edge border-r bg-surface md:block">
        <Sidebar />
      </aside>
      <Dialog open={menuOpen()} onOpenChange={setMenuOpen}>
        <Dialog.Portal>
          <Dialog.Overlay class="fixed inset-0 z-40 bg-canvas/70 backdrop-blur-sm md:hidden" />
          <Dialog.Content class="fixed inset-y-0 left-0 z-50 w-72 max-w-[85vw] border-edge border-r bg-surface shadow-2xl md:hidden">
            <Dialog.Title class="sr-only">{strings.nav.label}</Dialog.Title>
            <Dialog.CloseButton
              aria-label={strings.nav.close}
              class="absolute top-2.5 right-2.5 grid size-9 place-items-center rounded-md text-fg-muted hover:bg-raised hover:text-fg"
            >
              <Icon icon={X} class="size-5" />
            </Dialog.CloseButton>
            <Sidebar />
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog>
      <div class="flex min-w-0 flex-1 flex-col">
        <TopBar onOpenMenu={() => setMenuOpen(true)} />
        <main class="flex-1 px-4 py-6 md:px-8">{props.children}</main>
      </div>
    </div>
  );
}
