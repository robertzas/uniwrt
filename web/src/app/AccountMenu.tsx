import * as Popover from "@kobalte/core/popover";
import { LogOut } from "lucide";
import { createSignal, Show, useContext } from "solid-js";
import { SessionContext } from "../lib/session";
import { strings } from "../lib/strings";
import { Button } from "../ui/Button";

/** The signed-in user, with sign-out, in a popover from the sidebar's footer. */
export function AccountMenu(props: { compact?: boolean }) {
  const session = useContext(SessionContext);
  const [leaving, setLeaving] = createSignal(false);
  const initial = () => session.user.username.charAt(0).toUpperCase();

  return (
    <Popover.Root placement={props.compact ? "right-end" : "top-start"} gutter={8}>
      <Popover.Trigger
        aria-label={strings.account.label}
        class={[
          "flex h-9 items-center gap-2.5 rounded-control text-fg-muted text-sm transition-colors hover:bg-raised hover:text-fg data-expanded:bg-raised data-expanded:text-fg",
          props.compact ? "w-full justify-center" : "w-full px-2",
        ]}
      >
        <span
          aria-hidden="true"
          class="grid size-6 shrink-0 place-items-center rounded-full bg-accent-fill font-semibold text-[11px] text-on-accent"
        >
          {initial()}
        </span>
        <Show when={!props.compact}>
          <span class="truncate">{session.user.username}</span>
        </Show>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content class="z-50 w-60 rounded-card border border-edge bg-surface p-3 shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
          <Popover.Title class="sr-only">{strings.account.label}</Popover.Title>
          <div class="px-1 pb-3">
            <p class="truncate font-medium text-sm">{session.user.username}</p>
            <p class="text-fg-subtle text-xs">{strings.account.role[session.user.role]}</p>
          </div>
          <Button
            icon={LogOut}
            class="w-full justify-start"
            disabled={leaving()}
            onClick={() => {
              setLeaving(true);
              void session.signOut();
            }}
          >
            {strings.account.signOut}
          </Button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
