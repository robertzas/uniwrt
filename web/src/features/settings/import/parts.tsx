import { useNavigate } from "@solidjs/router";
import { CircleCheck } from "lucide";
import { createSignal, For, Show } from "solid-js";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";

const t = strings.settings.import;

/** A preview's errors (an alert) or warnings, each with its path; past `limit`, on demand. */
export function Issues(props: {
  title: string;
  tone: "crit" | "warn";
  issues: readonly { path: string; message: string }[];
  limit?: number;
}) {
  const [all, setAll] = createSignal(false);
  const shown = () =>
    all() || props.limit === undefined ? props.issues : props.issues.slice(0, props.limit);
  return (
    <div
      role={props.tone === "crit" ? "alert" : undefined}
      class={[
        "border-t px-4 py-3 text-sm",
        props.tone === "crit" ? "border-crit/40 bg-crit/10" : "border-warn/40 bg-warn/10",
      ]}
    >
      <p class="font-medium text-fg">{props.title}</p>
      <ul class="mt-1 flex flex-col gap-0.5 text-fg-muted">
        <For each={shown()}>
          {(issue) => (
            <li>
              <span class="font-mono text-fg-subtle text-xs">{issue.path}</span> {issue.message}
            </li>
          )}
        </For>
      </ul>
      <Show when={shown().length < props.issues.length}>
        <button
          type="button"
          class="mt-1.5 font-medium text-accent text-xs hover:underline"
          onClick={() => setAll(true)}
        >
          {t.showAll(props.issues.length)}
        </button>
      </Show>
    </div>
  );
}

/**
 * An accepted import: the revision it saved (`id`; the `base` when it saved nothing), with the
 * way on to its review.
 */
export function ImportDone(props: { id: number; base: number; label: string; body: string }) {
  const navigate = useNavigate();
  return (
    <Card aria-label={props.label}>
      <div role="status" class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-start">
        <Icon icon={CircleCheck} class="mt-0.5 size-5 shrink-0 text-ok" />
        <div class="min-w-0 flex-1">
          <p class="font-medium text-fg">
            {props.id === props.base ? t.done.none : t.done.title(props.id)}
          </p>
          <Show when={props.id !== props.base}>
            <p class="mt-1 text-fg-muted text-sm">{props.body}</p>
          </Show>
        </div>
        <div class="flex shrink-0 flex-wrap gap-2">
          <Button variant="ghost" size="sm" onClick={() => navigate("/settings?tab=revisions")}>
            {t.done.revisions}
          </Button>
          <Button variant="primary" size="sm" onClick={() => navigate("/changes")}>
            {t.done.review}
          </Button>
        </div>
      </div>
    </Card>
  );
}
