import { useLocation, useNavigate, useParams } from "@solidjs/router";
import { ChevronLeft } from "lucide";
import { createEffect, For, Show } from "solid-js";
import { HELP_INDEX, HELP_PATH, helpHref } from "../../lib/help";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";
import { Article } from "./Article";
import { ChapterStatus } from "./ChapterStatus";
import { createGuide } from "./guide";

const t = strings.help;

/**
 * `/help` and `/help/:chapter` (D-151): the help center, the user guide of this version, offline.
 * The chapters on the left, the chapter, and its sections on the right of wide screens.
 */
export function HelpPage() {
  const params = useParams<{ chapter?: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const slug = () => params.chapter ?? HELP_INDEX;
  const guide = createGuide(slug);
  const sections = () =>
    guide
      .chapters()
      .find((c) => c.slug === slug())
      ?.sections.filter((s) => s.depth === 2) ?? [];

  createEffect(
    () => guide.title() ?? t.center,
    (title) => {
      document.title = `${title} · ${t.center} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-7xl gap-10">
      <nav
        aria-label={t.chapters}
        class="sticky top-20 hidden max-h-[calc(100dvh-6rem)] w-56 shrink-0 self-start overflow-y-auto lg:block"
      >
        <p class="px-3 pb-2 font-medium text-[11px] text-fg-subtle uppercase tracking-wider">
          {t.chapters}
        </p>
        <ul class="flex flex-col gap-0.5">
          <For each={guide.chapters()}>
            {(chapter) => (
              <li>
                <a
                  href={helpHref({ chapter: chapter.slug })}
                  aria-current={chapter.slug === slug() ? "page" : undefined}
                  class="block rounded-control px-3 py-1.5 text-fg-muted text-sm hover:bg-raised hover:text-fg aria-[current=page]:bg-raised aria-[current=page]:font-medium aria-[current=page]:text-fg"
                >
                  {chapter.slug === HELP_INDEX ? t.contents : chapter.title}
                </a>
              </li>
            )}
          </For>
        </ul>
      </nav>
      <div class="min-w-0 max-w-3xl flex-1">
        <Show when={slug() !== HELP_INDEX}>
          <a
            href={HELP_PATH}
            class="-ml-1 mb-4 inline-flex items-center gap-1 text-fg-muted text-sm hover:text-fg lg:hidden"
          >
            <Icon icon={ChevronLeft} class="size-4" />
            {t.allChapters}
          </a>
        </Show>
        <ChapterStatus guide={guide}>
          {(html) => (
            <Article
              html={html()}
              anchor={location.hash.slice(1) || undefined}
              onNavigate={(topic) => navigate(helpHref(topic))}
            />
          )}
        </ChapterStatus>
      </div>
      <Show when={sections().length > 1}>
        <nav
          aria-label={t.onThisPage}
          class="sticky top-20 hidden max-h-[calc(100dvh-6rem)] w-52 shrink-0 self-start overflow-y-auto xl:block"
        >
          <p class="pb-2 font-medium text-[11px] text-fg-subtle uppercase tracking-wider">
            {t.onThisPage}
          </p>
          <ul class="flex flex-col gap-1 border-edge border-l">
            <For each={sections()}>
              {(section) => (
                <li>
                  <a
                    href={helpHref({ chapter: slug(), anchor: section.anchor })}
                    class="-ml-px block border-transparent border-l py-0.5 pl-3 text-fg-muted text-sm hover:border-edge-strong hover:text-fg"
                  >
                    {section.text}
                  </a>
                </li>
              )}
            </For>
          </ul>
        </nav>
      </Show>
    </div>
  );
}
