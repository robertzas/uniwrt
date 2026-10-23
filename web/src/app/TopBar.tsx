import { useLocation } from "@solidjs/router";
import { Menu } from "lucide-solid";
import { strings } from "../lib/strings";
import { HealthBadge } from "./HealthBadge";
import { findNavItem } from "./nav";

export function TopBar(props: { onOpenMenu: () => void }) {
  const location = useLocation();
  const title = () => findNavItem(location.pathname)?.label ?? strings.page.notFound.title;

  return (
    <header class="sticky top-0 z-30 flex h-14 shrink-0 items-center gap-3 border-edge border-b bg-canvas/80 px-4 backdrop-blur md:px-8">
      <button
        type="button"
        onClick={() => props.onOpenMenu()}
        aria-label={strings.nav.open}
        class="-ml-1.5 grid size-9 place-items-center rounded-md text-fg-muted hover:bg-raised hover:text-fg md:hidden"
      >
        <Menu class="size-5" aria-hidden="true" />
      </button>
      <span class="truncate font-medium text-sm">{title()}</span>
      <div class="ml-auto">
        <HealthBadge />
      </div>
    </header>
  );
}
