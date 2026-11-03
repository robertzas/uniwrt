import { KeyRound, Plus, Trash2 } from "lucide";
import { createUniqueId, For, Show } from "solid-js";
import type { Placeholder } from "../../../api/gen/Placeholder";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { FormField, INPUT, SELECT } from "../../../ui/FormField";
import { Icon } from "../../../ui/Icon";
import { type Draft, type DraftErrors, FORMATS, MAX_HEADERS, newHeader, withFormat } from "./draft";
import { SecretField } from "./SecretField";
import { TemplateEditor } from "./TemplateEditor";

const t = strings.settings.notifications.editor;
const formats = strings.settings.notifications.formats;
/** A header's name: a full row on phones, a fixed column beside its value from `sm`. */
const NAME_INPUT = `${INPUT.replace("w-full", "")} w-full border-edge font-mono sm:w-40 sm:shrink-0`;

/**
 * A webhook: its format (a preset or a custom template), its URL and headers, which are secrets
 * since services put their tokens in them.
 */
export function WebhookFields(props: {
  draft: Draft;
  errors: DraftErrors;
  /** The stored URL as its scheme and host; `false` when none (a new webhook). */
  storedUrl: string | false;
  placeholders: readonly Placeholder[];
  linkUrl: string | null;
  onChange: (change: Partial<Draft>) => void;
}) {
  const id = createUniqueId();
  const d = () => props.draft;
  const header = (key: number, change: Partial<Draft["headers"][number]>) =>
    props.onChange({
      headers: d().headers.map((h) => (h.key === key ? { ...h, ...change } : h)),
    });

  return (
    <div class="flex flex-col gap-4">
      <FormField
        label={t.format}
        for={`${id}-format`}
        hint={formats[d().format].hint}
        class="sm:max-w-sm"
      >
        <select
          id={`${id}-format`}
          class={SELECT}
          value={d().format}
          onChange={(event) =>
            props.onChange(withFormat(d(), event.currentTarget.value as Draft["format"]))
          }
        >
          <For each={FORMATS}>
            {(format) => <option value={format}>{formats[format].label}</option>}
          </For>
        </select>
      </FormField>
      <SecretField
        label={t.url}
        value={d().url}
        stored={props.storedUrl}
        onChange={(url) => props.onChange({ url })}
        plain
        placeholder={formats[d().format].example}
        hint={t.urlHint}
        error={props.errors.url}
      />
      <fieldset class="flex flex-col gap-2">
        <legend class="mb-1.5 font-medium text-fg text-sm">{t.headers}</legend>
        <For each={d().headers}>
          {(h) => (
            <div class="flex flex-wrap items-center gap-2 sm:flex-nowrap">
              <input
                type="text"
                value={h.name}
                readonly={h.stored}
                onInput={(event) => header(h.key, { name: event.currentTarget.value })}
                placeholder={t.headerName}
                aria-label={t.headerName}
                autocomplete="off"
                spellcheck={false}
                class={[NAME_INPUT, { "bg-raised": h.stored }]}
              />
              <Show
                when={h.value !== null}
                fallback={
                  <>
                    <div class="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-control border border-edge bg-raised px-2.5 text-fg-muted text-sm">
                      <Icon icon={KeyRound} class="size-3.5 shrink-0 text-fg-subtle" />
                      {strings.settings.notifications.secret.set}
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      variant="secondary"
                      aria-label={`${strings.settings.notifications.secret.replace}: ${h.name}`}
                      onClick={() => header(h.key, { value: "" })}
                    >
                      {strings.settings.notifications.secret.replace}
                    </Button>
                  </>
                }
              >
                <input
                  type="password"
                  value={h.value ?? ""}
                  onInput={(event) => header(h.key, { value: event.currentTarget.value })}
                  placeholder={t.headerValue}
                  aria-label={t.headerValueOf(h.name)}
                  autocomplete="new-password"
                  spellcheck={false}
                  class={[INPUT, "min-w-0 flex-1 basis-40 border-edge font-mono"]}
                />
              </Show>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                icon={Trash2}
                aria-label={t.removeHeader(h.name)}
                onClick={() =>
                  props.onChange({ headers: d().headers.filter((x) => x.key !== h.key) })
                }
              />
            </div>
          )}
        </For>
        <p class="text-crit text-xs" hidden={!props.errors.headers}>
          {props.errors.headers}
        </p>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          icon={Plus}
          class="self-start"
          disabled={d().headers.length >= MAX_HEADERS}
          onClick={() => props.onChange({ headers: [...d().headers, newHeader()] })}
        >
          {t.addHeader}
        </Button>
      </fieldset>
      <Show when={d().format === "custom"}>
        <TemplateEditor
          value={d().template}
          placeholders={props.placeholders}
          linkUrl={props.linkUrl}
          error={props.errors.template}
          onChange={(template) => props.onChange({ template })}
        />
      </Show>
    </div>
  );
}
