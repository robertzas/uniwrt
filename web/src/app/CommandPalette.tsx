import { CornerDownLeft, Search } from "lucide";
import { createEffect, createMemo, createSignal, createUniqueId, For, Show } from "solid-js";
import { strings } from "../lib/strings";
import { Dialog } from "../ui/Dialog";
import { Icon } from "../ui/Icon";
import { Kbd } from "../ui/Kbd";
import { type Command, filterCommands } from "./commands";

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  commands: readonly Command[];
}

/**
 * ⌘K: find a page or run a command. A combobox over a listbox (WAI-ARIA APG): focus stays in the
 * input, arrow keys move the active option, Enter runs it, and the dialog closes.
 */
export function CommandPalette(props: CommandPaletteProps) {
  const id = createUniqueId();
  const [query, setQuery] = createSignal("");
  const [active, setActive] = createSignal(0);
  const results = createMemo(() => filterCommands(props.commands, query()));
  const optionId = (index: number) => `${id}-option-${index}`;
  let list: HTMLDivElement | undefined;
  let input: HTMLInputElement | undefined;

  createEffect(
    () => props.open,
    (open) => {
      if (open) {
        setQuery("");
        setActive(0);
      }
    },
  );
  createEffect(active, (index) => {
    list?.querySelector(`#${CSS.escape(optionId(index))}`)?.scrollIntoView({ block: "nearest" });
  });

  const run = (command: Command | undefined) => {
    if (!command) return;
    props.onOpenChange(false);
    command.run();
  };
  const move = (by: number) => {
    const count = results().length;
    if (count > 0) setActive((i) => (i + by + count) % count);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    const keys: Record<string, () => void> = {
      ArrowDown: () => move(1),
      ArrowUp: () => move(-1),
      Home: () => setActive(0),
      End: () => setActive(Math.max(results().length - 1, 0)),
      Enter: () => run(results()[active()]),
    };
    const action = keys[event.key];
    if (action) {
      event.preventDefault();
      action();
    }
  };
  const groupStarts = (index: number) => results()[index - 1]?.group !== results()[index]?.group;

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={strings.palette.title}
      hideTitle
      initialFocus={() => input}
      class="max-w-xl overflow-hidden"
    >
      <div class="flex items-center gap-3 border-edge border-b px-4">
        <Icon icon={Search} class="size-4 shrink-0 text-fg-subtle" />
        <input
          ref={input}
          role="combobox"
          aria-expanded="true"
          aria-controls={`${id}-list`}
          aria-autocomplete="list"
          aria-activedescendant={results().length > 0 ? optionId(active()) : undefined}
          aria-label={strings.palette.title}
          placeholder={strings.palette.placeholder}
          autocomplete="off"
          spellcheck={false}
          value={query()}
          onInput={(event) => {
            setQuery(event.currentTarget.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
          class="h-12 min-w-0 flex-1 bg-transparent pr-8 text-base outline-none placeholder:text-fg-subtle"
        />
      </div>
      <div
        ref={list}
        id={`${id}-list`}
        role="listbox"
        aria-label={strings.palette.title}
        class="max-h-[min(24rem,60vh)] overflow-y-auto overscroll-contain p-2"
      >
        <Show
          when={results().length > 0}
          fallback={
            <p class="px-3 py-8 text-center text-fg-subtle text-sm">
              {strings.palette.empty(query().trim())}
            </p>
          }
        >
          <For each={results()}>
            {(command, index) => (
              <>
                <Show when={groupStarts(index())}>
                  <div
                    role="presentation"
                    class="px-3 pt-2 pb-1 font-medium text-[11px] text-fg-subtle uppercase tracking-wider"
                  >
                    {command.group}
                  </div>
                </Show>
                {/* biome-ignore lint/a11y/useKeyWithClickEvents: keys go to the combobox input, which points here with aria-activedescendant (APG). */}
                {/* biome-ignore lint/a11y/useFocusableInteractive: it has tabindex; Biome only knows React's tabIndex. */}
                <div
                  id={optionId(index())}
                  role="option"
                  tabindex={-1}
                  aria-selected={active() === index() ? "true" : "false"}
                  onPointerMove={() => {
                    setActive(index());
                  }}
                  onClick={() => run(command)}
                  class="flex h-9 cursor-pointer items-center gap-3 rounded-control px-3 text-fg-muted text-sm aria-selected:bg-raised aria-selected:text-fg"
                >
                  <Icon icon={command.icon} class="size-4 shrink-0" />
                  <span class="truncate">{command.label}</span>
                  <Show when={active() === index()}>
                    <Icon icon={CornerDownLeft} class="ml-auto size-3.5 text-fg-subtle" />
                  </Show>
                </div>
              </>
            )}
          </For>
        </Show>
      </div>
      <div class="hidden items-center gap-4 border-edge border-t px-4 py-2 text-fg-subtle text-xs sm:flex">
        <span class="flex items-center gap-1.5">
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
          {strings.palette.hint.move}
        </span>
        <span class="flex items-center gap-1.5">
          <Kbd>↵</Kbd>
          {strings.palette.hint.run}
        </span>
        <span class="flex items-center gap-1.5">
          <Kbd>Esc</Kbd>
          {strings.palette.hint.close}
        </span>
      </div>
    </Dialog>
  );
}
