import { CircleCheck, TriangleAlert } from "lucide";
import { Show } from "solid-js";
import type { JobError } from "../../../api/gen/JobError";
import { strings } from "../../../lib/strings";
import { Icon } from "../../../ui/Icon";

/** Why a job failed, in words the user can act on, with the server's message below. */
export function JobErrorNote(props: { error: JobError; compact?: boolean }) {
  const explained = () => strings.adopt.error[props.error.code];
  return (
    <div
      role="alert"
      class="flex items-start gap-2.5 rounded-control border border-crit/40 bg-crit/10 px-3 py-2.5 text-sm"
    >
      <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-crit" />
      <div class="min-w-0">
        <p class="font-medium text-fg">{explained().title}</p>
        <Show when={!props.compact}>
          <p class="mt-0.5 text-fg-muted">{explained().hint}</p>
        </Show>
        <p class="mt-1 break-words font-mono text-fg-subtle text-xs">{props.error.message}</p>
      </div>
    </div>
  );
}

/** The note that a job succeeded. */
export function JobSuccessNote(props: { text: string }) {
  return (
    <p
      role="status"
      class="flex items-start gap-2.5 rounded-control border border-ok/40 bg-ok/10 px-3 py-2.5 text-fg text-sm"
    >
      <Icon icon={CircleCheck} class="mt-0.5 size-4 shrink-0 text-ok" />
      {props.text}
    </p>
  );
}

/** A request's error before any job started (a probe, a busy target). */
export function RequestError(props: { text: string | undefined }) {
  return (
    <Show when={props.text}>
      {(text) => (
        <p
          role="alert"
          class="flex items-start gap-2 rounded-control border border-crit/40 bg-crit/10 px-3 py-2 text-fg text-sm"
        >
          <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-crit" />
          <span class="min-w-0 break-words">{text()}</span>
        </p>
      )}
    </Show>
  );
}
