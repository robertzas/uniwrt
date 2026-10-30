import { useLocation } from "@solidjs/router";
import { Menu, Search } from "lucide";
import { paletteShortcut } from "../lib/platform";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { Kbd } from "../ui/Kbd";
import { HealthBadge } from "./HealthBadge";
import { findNavItem } from "./nav";

export function TopBar(props: { onOpenMenu: () => void; onOpenPalette: () => void }) {
  const location = useLocation();
  const title = () => findNavItem(location.pathname)?.label ?? strings.page.notFound.title;

  return (
    <header class="sticky top-0 z-30 flex h-14 shrink-0 items-center gap-3 border-edge border-b bg-canvas/80 px-4 backdrop-blur md:px-8 glass:bg-transparent glass:backdrop-blur-panel">
      <button
        type="button"
        onClick={() => props.onOpenMenu()}
        aria-label={strings.nav.open}
        class="-ml-1.5 grid size-9 place-items-center rounded-control text-fg-muted hover:bg-raised hover:text-fg md:hidden"
      >
        <Icon icon={Menu} class="size-5" />
      </button>
      <span class="truncate font-medium text-sm">{title()}</span>
      <div class="ml-auto flex items-center gap-2">
        <button
          type="button"
          onClick={() => props.onOpenPalette()}
          aria-label={strings.palette.open}
          aria-keyshortcuts={paletteShortcut === "⌘K" ? "Meta+K" : "Control+K"}
          class="flex h-8 items-center gap-2 rounded-control border border-edge bg-surface px-2.5 text-fg-subtle text-sm transition-colors hover:bg-raised hover:text-fg max-sm:w-8 max-sm:justify-center sm:w-56"
        >
          <Icon icon={Search} class="size-4 shrink-0" />
          <span class="hidden sm:inline">{strings.palette.search}</span>
          <Kbd class="ml-auto hidden sm:inline-flex">{paletteShortcut}</Kbd>
        </button>
        <HealthBadge />
      </div>
    </header>
  );
}
