import { For } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { Wordmark } from "../ui/Wordmark";
import { isGroup, type NavGroup, type NavItem, nav } from "./nav";
import { ThemeToggle } from "./ThemeToggle";

/** Wordmark, navigation and the theme switch; shared by the desktop rail and the mobile drawer. */
export function Sidebar() {
  return (
    <div class="flex h-full flex-col">
      <div class="flex h-14 shrink-0 items-center px-4">
        <a href="/" class="rounded-md">
          <Wordmark />
        </a>
      </div>
      <nav aria-label={strings.nav.label} class="flex-1 overflow-y-auto px-2 pb-4">
        <ul class="space-y-0.5">
          <For each={nav}>
            {(entry) =>
              isGroup(entry) ? (
                <Group group={entry} />
              ) : (
                <li>
                  <NavLink item={entry} />
                </li>
              )
            }
          </For>
        </ul>
      </nav>
      <div class="border-edge border-t px-4 py-3">
        <ThemeToggle />
      </div>
    </div>
  );
}

function Group(props: { group: NavGroup }) {
  return (
    <li class="pt-4">
      <div class="flex h-7 items-center gap-2 px-2.5 font-medium text-[11px] text-fg-subtle uppercase tracking-wider">
        {props.group.label}
      </div>
      <ul aria-label={props.group.label} class="space-y-0.5">
        <For each={props.group.items}>
          {(item) => (
            <li>
              <NavLink item={item} />
            </li>
          )}
        </For>
      </ul>
    </li>
  );
}

function NavLink(props: { item: NavItem }) {
  return (
    <a
      href={props.item.path}
      class="flex h-8 items-center gap-2.5 rounded-md px-2.5 text-fg-muted text-sm transition-colors hover:bg-raised/60 hover:text-fg data-active:bg-raised data-active:font-medium data-active:text-fg"
    >
      <Icon icon={props.item.icon} class="size-4 shrink-0" />
      <span class="truncate">{props.item.label}</span>
    </a>
  );
}
