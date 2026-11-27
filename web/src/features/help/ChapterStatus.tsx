import type { JSX } from "@solidjs/web";
import { Show } from "solid-js";
import { HELP_PATH } from "../../lib/help";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import type { createGuide } from "./guide";

const t = strings.help;

/** A chapter once loaded; while it loads, a line; a missing or failed one, why, and a way on. */
export function ChapterStatus(props: {
  guide: ReturnType<typeof createGuide>;
  children: (html: () => string) => JSX.Element;
}) {
  const html = () => {
    const c = props.guide.chapter();
    return c.state === "ready" ? c.html : undefined;
  };
  const state = () => props.guide.chapter().state;
  return (
    <Show when={html()} fallback={<Fallback state={state()} retry={props.guide.retry} />}>
      {(value) => props.children(value)}
    </Show>
  );
}

function Fallback(props: { state: string; retry: () => void }) {
  return (
    <Show
      when={props.state !== "loading"}
      fallback={<p class="py-8 text-center text-fg-subtle text-sm">{t.loading}</p>}
    >
      <div role="alert" class="mx-auto max-w-sm py-10 text-center">
        <p class="font-medium text-sm">
          {props.state === "missing" ? t.missing.title : t.failed.title}
        </p>
        <p class="mt-1 text-fg-muted text-sm">
          {props.state === "missing" ? t.missing.description : t.failed.description}
        </p>
        <Show
          when={props.state === "failed"}
          fallback={
            <a href={HELP_PATH} class="mt-4 inline-block font-medium text-accent text-sm">
              {t.missing.contents}
            </a>
          }
        >
          <Button class="mt-4" onClick={() => props.retry()}>
            {t.failed.retry}
          </Button>
        </Show>
      </div>
    </Show>
  );
}
