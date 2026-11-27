import { ArrowLeft, Maximize2, X } from "lucide";
import { createEffect, createSignal, onSettled, Show } from "solid-js";
import { type HelpTopic, helpHref } from "../../lib/help";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";
import { Article } from "./Article";
import { ChapterStatus } from "./ChapterStatus";
import { createGuide } from "./guide";

const t = strings.help;

export interface HelpPanelProps {
  /** The page's section (or what the palette asked for); the panel follows it. */
  topic: HelpTopic;
  onClose: () => void;
}

const button =
  "grid size-8 shrink-0 place-items-center rounded-control text-fg-muted hover:bg-raised hover:text-fg";

/**
 * Help beside the page (D-151): the guide at the page's section, in a column on the right of wide
 * screens and a sheet from the bottom edge below `lg`; the page stays usable. Its links move
 * through the guide in the panel, **Back** returns, and the help center opens the same section as
 * a page. Escape closes it.
 */
export function HelpPanel(props: HelpPanelProps) {
  const [trail, setTrail] = createSignal<readonly HelpTopic[]>([]);
  const current = () => trail().at(-1) ?? props.topic;
  const guide = createGuide(() => current().chapter);
  let panel: HTMLElement | undefined;
  let body: HTMLDivElement | undefined;

  // Another page (or a palette entry) starts the panel over at its section.
  createEffect(
    () => props.topic,
    () => {
      setTrail([]);
    },
    { defer: true },
  );
  onSettled(() => panel?.focus({ preventScroll: true }));

  return (
    <aside
      ref={panel}
      data-help-panel
      tabindex={-1}
      aria-label={t.title}
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.defaultPrevented) {
          event.preventDefault();
          props.onClose();
        }
      }}
      class="z-40 flex flex-col border-edge bg-surface outline-none glass:backdrop-blur-panel max-lg:fixed max-lg:inset-x-0 max-lg:bottom-0 max-lg:h-[min(75dvh,44rem)] max-lg:animate-sheet-in max-lg:rounded-t-card max-lg:border-t max-lg:pb-[env(safe-area-inset-bottom)] max-lg:shadow-overlay lg:sticky lg:top-0 lg:h-dvh lg:w-[26rem] lg:shrink-0 lg:border-l"
    >
      <header class="flex h-14 shrink-0 items-center gap-1 border-edge border-b px-2">
        <Show when={trail().length > 0}>
          <button
            type="button"
            aria-label={t.back}
            title={t.back}
            class={button}
            onClick={() => setTrail((list) => list.slice(0, -1))}
          >
            <Icon icon={ArrowLeft} class="size-4" />
          </button>
        </Show>
        <div class="min-w-0 flex-1 px-2">
          <h2 class="font-semibold text-sm leading-tight">{t.title}</h2>
          <p class="h-4 truncate text-fg-subtle text-xs">{guide.title()}</p>
        </div>
        <a
          href={helpHref(current())}
          aria-label={t.openCenter}
          title={t.openCenter}
          class={button}
          onClick={() => props.onClose()}
        >
          <Icon icon={Maximize2} class="size-4" />
        </a>
        <button
          type="button"
          aria-label={t.close}
          title={t.close}
          class={button}
          onClick={() => props.onClose()}
        >
          <Icon icon={X} class="size-4" />
        </button>
      </header>
      <div ref={body} class="min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-4">
        <ChapterStatus guide={guide}>
          {(html) => (
            <Article
              html={html()}
              anchor={current().anchor}
              scroller={() => body}
              onNavigate={(topic) => setTrail((list) => [...list, topic])}
            />
          )}
        </ChapterStatus>
      </div>
    </aside>
  );
}
