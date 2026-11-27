import { createEffect, createSignal } from "solid-js";
import type { HelpChapter } from "../../lib/help";

/**
 * The guide's index and chapters, from the build (`web/guide/plugin.ts`). Each loads on first use,
 * in a chunk of its own; a failed load (a controller upgraded under an open page) is tried again
 * next time.
 */
let index: Promise<readonly HelpChapter[]> | undefined;

export function guideIndex(): Promise<readonly HelpChapter[]> {
  index ??= import("virtual:uniwrt-guide").then(
    (m) => m.chapters,
    (error: unknown) => {
      index = undefined;
      throw error;
    },
  );
  return index;
}

/** A chapter's state: loading, its HTML, a chapter the guide doesn't have, or a failed load. */
export type ChapterState =
  | { state: "loading" }
  | { state: "ready"; html: string }
  | { state: "missing" }
  | { state: "failed" };

/**
 * The chapter `slug()` names, loaded into a plain signal (nothing waits on it, D-048), and the
 * index; `retry` loads a failed chapter again.
 */
export function createGuide(slug: () => string) {
  const [chapters, setChapters] = createSignal<readonly HelpChapter[]>([]);
  const [chapter, setChapter] = createSignal<ChapterState>({ state: "loading" });
  const [attempt, setAttempt] = createSignal(0);
  guideIndex().then(setChapters, () => {});
  createEffect(
    () => [slug(), attempt()] as const,
    ([name]) => {
      let live = true;
      setChapter({ state: "loading" });
      import("virtual:uniwrt-guide")
        .then((m) => m.loadChapter(name))
        .then(
          (html) =>
            live &&
            setChapter(html === undefined ? { state: "missing" } : { state: "ready", html }),
          () => live && setChapter({ state: "failed" }),
        );
      return () => {
        live = false;
      };
    },
  );
  return {
    chapters,
    chapter,
    title: () => chapters().find((c) => c.slug === slug())?.title,
    retry: () => setAttempt((n) => n + 1),
  };
}
