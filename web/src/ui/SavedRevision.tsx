import { CircleCheck } from "lucide";
import { Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "./Icon";

const t = strings.revision;

/**
 * A change saved as the site's next revision, with the link to review and apply it
 * (`/changes`). `note` adds what the apply will also do.
 */
export function SavedRevision(props: { revision: number; note?: string | undefined }) {
  return (
    <div
      class="flex flex-wrap items-center gap-3 rounded-control border border-ok/40 bg-ok/10 px-3 py-2.5 text-sm"
      role="status"
    >
      <Icon icon={CircleCheck} class="size-4 shrink-0 text-ok" />
      <div class="min-w-0 flex-1">
        <p>{t.saved(props.revision)}</p>
        <Show when={props.note}>
          <p class="mt-0.5 text-fg-muted text-xs">{props.note}</p>
        </Show>
      </div>
      <a
        href="/changes"
        class="inline-flex h-8 items-center rounded-control border border-edge bg-surface px-3 font-medium text-fg text-sm hover:bg-raised"
      >
        {t.review}
      </a>
    </div>
  );
}
