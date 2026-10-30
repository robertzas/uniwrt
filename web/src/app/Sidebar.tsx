import { For, Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { Tooltip } from "../ui/Tooltip";
import { Wordmark } from "../ui/Wordmark";
import { AppearanceMenu } from "./AppearanceMenu";
import { isGroup, type NavGroup, type NavItem, nav } from "./nav";

/**
 * Wordmark, navigation and the appearance menu; shared by the desktop sidebar and the mobile
 * drawer. As a `rail` (the UniFi-familiar skin) it shows icons only, each with a tooltip; the
 * labels stay in the accessibility tree.
 */
export function Sidebar(props: { rail?: boolean }) {
  return (
    <div class="flex h-full flex-col">
      <div class={["flex h-14 shrink-0 items-center", props.rail ? "justify-center" : "px-4"]}>
        <a href="/" class="rounded-control">
          <Wordmark compact={props.rail} />
        </a>
      </div>
      <nav
        aria-label={strings.nav.label}
        class={["flex-1 overflow-y-auto pb-4", props.rail ? "px-3" : "px-2"]}
      >
        <ul class={props.rail ? "space-y-1" : "space-y-0.5"}>
          <For each={nav}>
            {(entry) =>
              isGroup(entry) ? (
                <Group group={entry} rail={props.rail} />
              ) : (
                <li>
                  <NavLink item={entry} rail={props.rail} />
                </li>
              )
            }
          </For>
        </ul>
      </nav>
      <div class={["border-edge border-t py-3", props.rail ? "px-3" : "px-2"]}>
        <AppearanceMenu compact={props.rail} />
      </div>
    </div>
  );
}

function Group(props: { group: NavGroup; rail?: boolean }) {
  return (
    <li class={props.rail ? "mt-2 border-edge border-t pt-2" : "pt-4"}>
      <div
        class={[
          "flex h-7 items-center gap-2 px-2.5 font-medium text-[11px] text-fg-subtle uppercase tracking-wider",
          { "sr-only": props.rail },
        ]}
      >
        {props.group.label}
      </div>
      <ul aria-label={props.group.label} class={props.rail ? "space-y-1" : "space-y-0.5"}>
        <For each={props.group.items}>
          {(item) => (
            <li>
              <NavLink item={item} rail={props.rail} />
            </li>
          )}
        </For>
      </ul>
    </li>
  );
}

const link =
  "flex items-center rounded-control text-fg-muted transition-colors hover:bg-raised/60 hover:text-fg data-active:bg-nav-active data-active:text-nav-active-fg glass:data-active:shadow-[inset_2px_0_0_var(--color-accent)]";

/** A page link. The router marks the current section with `data-active` (a prefix match). */
function NavLink(props: { item: NavItem; rail?: boolean }) {
  return (
    <Show
      when={props.rail}
      fallback={
        <a
          href={props.item.path}
          class={[link, "h-8 gap-2.5 px-2.5 text-sm data-active:font-medium"]}
        >
          <Icon icon={props.item.icon} class="size-4 shrink-0" />
          <span class="truncate">{props.item.label}</span>
        </a>
      }
    >
      <Tooltip
        as="a"
        href={props.item.path}
        content={props.item.label}
        placement="right"
        class={[link, "h-10 justify-center"].join(" ")}
      >
        <Icon icon={props.item.icon} class="size-5" />
        <span class="sr-only">{props.item.label}</span>
      </Tooltip>
    </Show>
  );
}
