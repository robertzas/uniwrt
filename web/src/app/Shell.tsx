import { useLocation, useNavigate } from "@solidjs/router";
import type { JSX } from "@solidjs/web";
import { createEffect, createMemo, createSignal, Errored, onSettled } from "solid-js";
import { currentSkin } from "../lib/appearance";
import { isPaletteShortcut } from "../lib/platform";
import { strings } from "../lib/strings";
import { Button } from "../ui/Button";
import { Drawer } from "../ui/Dialog";
import { Toaster } from "../ui/Toast";
import { CommandPalette } from "./CommandPalette";
import { appCommands } from "./commands";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";

/**
 * The app frame: a sidebar (an icon rail in the UniFi-familiar skin) on wide screens, a drawer on
 * phones, the top bar, the ⌘K palette and the toast region. An error in a page stays in its pane.
 */
export function Shell(props: { children?: JSX.Element }) {
  const [menuOpen, setMenuOpen] = createSignal(false);
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const location = useLocation();
  const navigate = useNavigate();
  const commands = createMemo(() => appCommands((path) => navigate(path)));
  const rail = () => currentSkin() === "unifi";

  createEffect(
    () => location.pathname,
    () => {
      setMenuOpen(false);
    },
    { defer: true },
  );
  onSettled(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isPaletteShortcut(event)) {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  return (
    <div class="flex min-h-dvh">
      <aside class="sticky top-0 hidden h-dvh w-sidebar shrink-0 border-edge border-r bg-surface md:block glass:backdrop-blur-panel">
        <Sidebar rail={rail()} />
      </aside>
      <Drawer
        open={menuOpen()}
        onOpenChange={setMenuOpen}
        side="left"
        title={strings.nav.label}
        hideTitle
        class="md:hidden"
      >
        <Sidebar />
      </Drawer>
      <div class="flex min-w-0 flex-1 flex-col">
        <TopBar onOpenMenu={() => setMenuOpen(true)} onOpenPalette={() => setPaletteOpen(true)} />
        <main class="flex-1 px-4 py-6 md:px-8">
          <Errored fallback={(_, reset) => <PageError onRetry={reset} />}>{props.children}</Errored>
        </main>
      </div>
      <CommandPalette open={paletteOpen()} onOpenChange={setPaletteOpen} commands={commands()} />
      <Toaster />
    </div>
  );
}

function PageError(props: { onRetry: () => void }) {
  const text = strings.page.error;
  return (
    <section role="alert" class="mx-auto flex max-w-md flex-col items-center py-20 text-center">
      <h1 class="font-semibold text-lg tracking-tight">{text.title}</h1>
      <p class="mt-2 text-fg-muted text-sm">{text.description}</p>
      <Button variant="primary" class="mt-6" onClick={() => props.onRetry()}>
        {text.retry}
      </Button>
    </section>
  );
}
