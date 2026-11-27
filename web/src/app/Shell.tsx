import { useLocation, useNavigate } from "@solidjs/router";
import { Dynamic, type JSX } from "@solidjs/web";
import {
  type Component,
  createEffect,
  createMemo,
  createSignal,
  Errored,
  onSettled,
  Show,
  useContext,
} from "solid-js";
import type { Client } from "../api/gen/Client";
import type { HelpPanelProps } from "../features/help/HelpPanel";
import { api } from "../lib/api";
import { currentSkin } from "../lib/appearance";
import { HELP_INDEX, type HelpChapter, type HelpTopic, helpTopic, isHelpPath } from "../lib/help";
import { LiveContext } from "../lib/live";
import { isPaletteShortcut } from "../lib/platform";
import { SessionContext } from "../lib/session";
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
 * phones, the top bar, the ⌘K palette, the help panel (D-151) and the toast region. An error in a
 * page stays in its pane.
 */
export function Shell(props: { children?: JSX.Element }) {
  const [menuOpen, setMenuOpen] = createSignal(false);
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const location = useLocation();
  const navigate = useNavigate();
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  // Read each time the palette opens, into a plain signal: nothing waits on it (D-048).
  const [clients, setClients] = createSignal<readonly Client[]>([]);
  createEffect(paletteOpen, (open) => {
    if (!open) return;
    const controller = new AbortController();
    api.clients
      .list(controller.signal)
      // Anything but a list (a proxy's error page, say) must not break the palette.
      .then((list) => setClients(Array.isArray(list) ? list : []))
      .catch(() => {});
    return () => controller.abort();
  });
  // Help beside the page: the panel's code and the guide load the first time it opens.
  const [helpOpen, setHelpOpen] = createSignal(false);
  const [helpAsked, setHelpAsked] = createSignal<HelpTopic>();
  const [HelpPanel, setHelpPanel] = createSignal<Component<HelpPanelProps>>();
  const [guide, setGuide] = createSignal<readonly HelpChapter[]>([]);
  const helpPage = () => isHelpPath(location.pathname);
  const pageTopic = createMemo(
    () => helpTopic(location.pathname, location.search) ?? { chapter: HELP_INDEX },
  );
  const openHelp = (topic?: HelpTopic) => {
    setHelpAsked(topic);
    setHelpOpen(true);
    if (!HelpPanel()) {
      import("../features/help/HelpPanel").then(
        (m) => setHelpPanel(() => m.HelpPanel),
        () => setHelpOpen(false),
      );
    }
  };
  const closeHelp = () => {
    const inPanel = document.activeElement?.closest("[data-help-panel]");
    setHelpOpen(false);
    if (inPanel) document.querySelector<HTMLElement>("[data-help-button]")?.focus();
  };
  createEffect(paletteOpen, (open) => {
    if (!open || guide().length > 0) return;
    import("../features/help/guide").then((m) => m.guideIndex()).then(setGuide, () => {});
  });
  const commands = createMemo(() =>
    appCommands({
      navigate: (path) => navigate(path),
      devices: live.fleet.nodes,
      clients: clients(),
      signOut: () => void session.signOut(),
      admin: session.user.role === "admin",
      openHelp,
      guide: guide(),
    }),
  );
  const rail = () => currentSkin() === "unifi";

  createEffect(
    () => location.pathname,
    () => {
      setMenuOpen(false);
    },
    { defer: true },
  );
  // The panel follows the page; the help center replaces it.
  createEffect(
    () => location.pathname + location.search,
    () => {
      setHelpAsked(undefined);
      if (helpPage()) setHelpOpen(false);
    },
    { defer: true },
  );
  onSettled(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isPaletteShortcut(event)) {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (isHelpShortcut(event) && !helpPage()) {
        event.preventDefault();
        if (helpOpen()) closeHelp();
        else openHelp();
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
        <TopBar
          onOpenMenu={() => setMenuOpen(true)}
          onOpenPalette={() => setPaletteOpen(true)}
          helpOpen={helpOpen()}
          onToggleHelp={() => (helpOpen() ? closeHelp() : openHelp())}
        />
        <main class="flex-1 px-4 py-6 md:px-8">
          <Errored fallback={(_, reset) => <PageError onRetry={reset} />}>{props.children}</Errored>
        </main>
      </div>
      <Show when={helpOpen() && !helpPage() && HelpPanel()}>
        {(panel) => (
          <Dynamic component={panel()} topic={helpAsked() ?? pageTopic()} onClose={closeHelp} />
        )}
      </Show>
      <CommandPalette open={paletteOpen()} onOpenChange={setPaletteOpen} commands={commands()} />
      <Toaster />
    </div>
  );
}

/** `?` outside text fields and dialogs opens or closes help. */
function isHelpShortcut(event: KeyboardEvent): boolean {
  if (event.key !== "?" || event.ctrlKey || event.metaKey || event.altKey) return false;
  const target = event.target;
  if (!(target instanceof HTMLElement)) return true;
  return !(
    target.isContentEditable ||
    /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) ||
    target.closest('[role="dialog"]')
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
