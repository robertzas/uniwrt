import { A } from "@solidjs/router";
import { For } from "solid-js";
import { Dynamic } from "solid-js/web";
import { strings } from "../lib/strings";
import { Wordmark } from "../ui/Wordmark";
import { isGroup, type NavGroup, type NavItem, nav } from "./nav";
import { ThemeToggle } from "./ThemeToggle";

/** Wordmark, navigation and the theme switch; shared by the desktop rail and the mobile drawer. */
export function Sidebar() {
  return (
    <div class="flex h-full flex-col">
      <div class="flex h-14 shrink-0 items-center px-4">
        <A href="/" class="rounded-md">
          <Wordmark />
        </A>
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
    <A
      href={props.item.path}
      end
      class="flex h-8 items-center gap-2.5 rounded-md px-2.5 text-sm transition-colors"
      activeClass="bg-raised font-medium text-fg"
      inactiveClass="text-fg-muted hover:bg-raised/60 hover:text-fg"
    >
      <Dynamic component={props.item.icon} class="size-4 shrink-0" aria-hidden="true" />
      <span class="truncate">{props.item.label}</span>
    </A>
  );
}
