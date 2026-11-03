import { CircleAlert } from "lucide";
import { createMemo, createSignal, createUniqueId, For, Show } from "solid-js";
import type { Placeholder } from "../../../api/gen/Placeholder";
import { strings } from "../../../lib/strings";
import { Icon } from "../../../ui/Icon";
import { Segmented } from "../../../ui/Segmented";
import { insertAt, MAX_TEMPLATE, renderTemplate, sampleAlert, sampleTest } from "./template";

const t = strings.settings.notifications.template;

/**
 * A custom webhook's JSON body: the placeholders to put in at the caret, and a preview of what
 * an alert (or a test) makes of it, as the server renders it.
 */
export function TemplateEditor(props: {
  value: string;
  placeholders: readonly Placeholder[];
  linkUrl: string | null;
  error?: string | undefined;
  onChange: (template: string) => void;
}) {
  const id = createUniqueId();
  const [sample, setSample] = createSignal<"alert" | "test">("alert");
  let area: HTMLTextAreaElement | undefined;
  // One instant per editor, so the preview doesn't change on its own.
  const at = Date.now();

  const preview = createMemo(() => {
    const values =
      sample() === "alert" ? sampleAlert(props.linkUrl, at) : sampleTest(props.linkUrl, at);
    const rendered = renderTemplate(props.value, props.placeholders, values);
    return "error" in rendered
      ? rendered
      : { text: JSON.stringify(JSON.parse(rendered.text), null, 2) };
  });
  const problem = () => {
    const shown = preview();
    return "error" in shown ? shown.error : undefined;
  };
  const text = () => {
    const shown = preview();
    return "text" in shown ? shown.text : "";
  };
  const bytes = createMemo(() => new TextEncoder().encode(props.value).length);

  const insert = (placeholder: Placeholder) => {
    const text = `{{${placeholder.name}}}`;
    const from = area?.selectionStart ?? props.value.length;
    const to = area?.selectionEnd ?? from;
    const next = insertAt(props.value, from, to, text);
    props.onChange(next.template);
    queueMicrotask(() => {
      area?.focus();
      area?.setSelectionRange(next.caret, next.caret);
    });
  };

  return (
    <div class="flex flex-col gap-3">
      <div class="flex flex-col gap-1.5">
        <div class="flex items-baseline justify-between gap-2">
          <label for={id} class="font-medium text-fg text-sm">
            {t.label}
          </label>
          <span
            class={[
              "font-mono text-[0.6875rem] tabular-nums",
              bytes() > MAX_TEMPLATE ? "text-crit" : "text-fg-subtle",
            ]}
          >
            {t.size(bytes(), MAX_TEMPLATE)}
          </span>
        </div>
        <textarea
          id={id}
          ref={(el) => {
            area = el;
          }}
          value={props.value}
          onInput={(event) => props.onChange(event.currentTarget.value)}
          rows={9}
          spellcheck={false}
          autocapitalize="none"
          aria-invalid={props.error ? "true" : undefined}
          aria-describedby={`${id}-help`}
          class={[
            "w-full resize-y rounded-control border bg-canvas px-2.5 py-2 font-mono text-fg text-xs leading-relaxed outline-none transition-[border-color,box-shadow] focus:border-accent focus:ring-2 focus:ring-accent/30",
            props.error ? "border-crit" : "border-edge",
          ]}
        />
        <p id={`${id}-help`} class="text-fg-subtle text-xs">
          {t.hint}
        </p>
      </div>
      <fieldset class="flex flex-wrap gap-1.5" aria-label={t.placeholders}>
        <For each={props.placeholders}>
          {(p) => (
            <button
              type="button"
              title={p.number ? t.numberHint : t.textHint}
              onClick={() => insert(p)}
              class={[
                "rounded-full border px-2 py-0.5 font-mono text-[0.6875rem] transition-colors hover:bg-raised",
                p.number ? "border-info/40 text-info" : "border-edge text-fg-muted",
              ]}
            >
              {`{{${p.name}}}`}
            </button>
          )}
        </For>
      </fieldset>
      <div class="overflow-hidden rounded-control border border-edge">
        <div class="flex flex-wrap items-center justify-between gap-2 border-edge border-b bg-raised px-3 py-1.5">
          <span class="font-medium text-fg-muted text-xs">{t.preview}</span>
          <Segmented
            label={t.preview}
            value={sample()}
            options={[
              { value: "alert", label: t.sampleAlert },
              { value: "test", label: t.sampleTest },
            ]}
            onChange={setSample}
          />
        </div>
        <Show
          when={problem()}
          fallback={
            <pre class="max-h-56 overflow-auto px-3 py-2 font-mono text-fg text-xs leading-relaxed">
              {text()}
            </pre>
          }
        >
          {(why) => (
            <p role="alert" class="flex items-start gap-1.5 px-3 py-3 text-sm">
              <Icon icon={CircleAlert} class="mt-0.5 size-4 shrink-0 text-crit" />
              {t.problem(why())}
            </p>
          )}
        </Show>
      </div>
    </div>
  );
}
