import { Plus, Trash2, Undo2 } from "lucide";
import { createSignal, For, Show, untrack } from "solid-js";
import type { UciTreeSection } from "../../../api/gen/UciTreeSection";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import { deltaOf, draftOf, isEmpty, newOption, type OptionDraft } from "./draft";
import type { SectionDelta } from "./edits";
import { isMaskedOption, isUciName, isUciType } from "./uci";

const t = strings.device.uci;
const e = t.errors;

const INPUT =
  "w-full rounded-control border bg-canvas px-2.5 py-1.5 font-mono text-fg text-xs outline-none transition-[border-color,box-shadow] placeholder:font-sans placeholder:text-fg-subtle focus:border-accent focus:ring-2 focus:ring-accent/30 disabled:opacity-60";

/** A new section's type and name as typed. */
export interface NewSection {
  type: string;
  name: string | null;
}

/**
 * Edits a section's options (`section`), or a new section's type, name and options: each
 * option's value (a list one item per line), a masked one typed anew or left to keep, options
 * deleted and added. `onSave` answers what went wrong, if anything.
 */
export function SectionEditor(props: {
  title: string;
  section?: UciTreeSection;
  /** The names the package's sections hold, which a new one can't take. */
  taken?: readonly string[];
  onSave: (delta: SectionDelta, added?: NewSection) => Promise<string | undefined>;
  onCancel: () => void;
}) {
  const [drafts, setDrafts] = createSignal<OptionDraft[]>(
    untrack(() => (props.section ? draftOf(props.section) : [newOption()])),
  );
  const [type, setType] = createSignal("");
  const [name, setName] = createSignal("");
  const [errors, setErrors] = createSignal<Record<number, string>>({});
  const [headErrors, setHeadErrors] = createSignal<{ type?: string; name?: string }>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const adding = () => props.section === undefined;

  const change = (i: number, next: Partial<OptionDraft>) => {
    setDrafts((all) => all.map((d, j) => (j === i ? { ...d, ...next } : d)));
    setErrors(({ [i]: _, ...rest }) => rest);
    setFailure(undefined);
  };
  const secret = (draft: OptionDraft) => (draft.read ? draft.secret : isMaskedOption(draft.name));

  const headOf = () => {
    const found: { type?: string; name?: string } = {};
    if (!isUciType(type().trim())) found.type = e.sectionType;
    const typed = name().trim();
    if (typed && !isUciName(typed)) found.name = e.sectionName;
    else if (typed && props.taken?.includes(typed)) found.name = e.sectionTaken(typed);
    return found;
  };

  const save = async () => {
    const { delta, errors } = deltaOf(props.section, drafts());
    const head = adding() ? headOf() : {};
    setErrors(errors);
    setHeadErrors(head);
    if (Object.keys(errors).length > 0 || head.type || head.name) return;
    if (!adding() && isEmpty(delta)) {
      setFailure(t.editor.noChange);
      return;
    }
    setBusy(true);
    const added = adding() ? { type: type().trim(), name: name().trim() || null } : undefined;
    const failed = await props.onSave(delta, added);
    setBusy(false);
    setFailure(failed);
  };

  return (
    <form
      class="flex flex-col gap-3"
      aria-label={props.title}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <Show when={adding()}>
        <div class="grid gap-3 sm:grid-cols-2">
          <TextField
            label={t.editor.type}
            value={type()}
            onInput={(value) => {
              setType(value);
              setHeadErrors(({ type: _, ...rest }) => rest);
            }}
            hint={t.editor.typeHint}
            error={headErrors().type}
            autofocus
          />
          <TextField
            label={t.editor.name}
            value={name()}
            onInput={(value) => {
              setName(value);
              setHeadErrors(({ name: _, ...rest }) => rest);
            }}
            hint={t.editor.nameHint}
            error={headErrors().name}
          />
        </div>
      </Show>
      <fieldset class="flex flex-col gap-2">
        <legend class="mb-1 font-medium text-sm">{t.editor.options}</legend>
        <For each={drafts()} keyed={false}>
          {(draft, i) => (
            <div
              class={[
                "grid grid-cols-[minmax(0,10rem)_minmax(0,1fr)_auto_auto] items-start gap-2",
                { "opacity-60": draft().deleted },
              ]}
            >
              <Show
                when={draft().read === undefined}
                fallback={
                  <span
                    class={[
                      "truncate pt-1.5 font-mono text-xs",
                      { "line-through": draft().deleted },
                    ]}
                    title={draft().name}
                  >
                    {draft().name}
                  </span>
                }
              >
                <input
                  class={[INPUT, errors()[i] ? "border-crit" : "border-edge"]}
                  aria-label={t.editor.option}
                  placeholder={t.editor.option}
                  value={draft().name}
                  autocapitalize="none"
                  spellcheck={false}
                  onInput={(event) => change(i, { name: event.currentTarget.value })}
                />
              </Show>
              <div class="flex min-w-0 flex-col gap-1">
                <Show
                  when={draft().list && !secret(draft())}
                  fallback={
                    <input
                      class={[INPUT, errors()[i] ? "border-crit" : "border-edge"]}
                      type={secret(draft()) ? "password" : "text"}
                      aria-label={t.editor.value}
                      placeholder={
                        secret(draft())
                          ? draft().read
                            ? t.editor.keepSecret
                            : t.editor.newSecret
                          : undefined
                      }
                      value={draft().text}
                      disabled={draft().deleted || (secret(draft()) && draft().list)}
                      autocomplete="off"
                      autocapitalize="none"
                      spellcheck={false}
                      onInput={(event) => change(i, { text: event.currentTarget.value })}
                    />
                  }
                >
                  <textarea
                    class={[INPUT, "min-h-16", errors()[i] ? "border-crit" : "border-edge"]}
                    aria-label={t.editor.value}
                    placeholder={t.editor.listHint}
                    value={draft().text}
                    disabled={draft().deleted}
                    rows={Math.min(8, Math.max(2, draft().text.split("\n").length))}
                    autocapitalize="none"
                    spellcheck={false}
                    onInput={(event) => change(i, { text: event.currentTarget.value })}
                  />
                </Show>
                <Show when={errors()[i]}>
                  {(why) => <span class="text-crit text-xs">{why()}</span>}
                </Show>
              </div>
              <label
                class="inline-flex h-8 items-center gap-1.5 text-fg-muted text-xs"
                title={t.editor.listHint}
              >
                <input
                  type="checkbox"
                  checked={draft().list}
                  disabled={draft().deleted || secret(draft())}
                  onChange={(event) => change(i, { list: event.currentTarget.checked })}
                />
                {t.editor.list}
              </label>
              <Show
                when={draft().deleted}
                fallback={
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={Trash2}
                    aria-label={t.editor.deleteOption(draft().name)}
                    title={t.editor.deleteOption(draft().name)}
                    onClick={() =>
                      draft().read === undefined
                        ? setDrafts((all) => all.filter((_, j) => j !== i))
                        : change(i, { deleted: true })
                    }
                  />
                }
              >
                <Button
                  size="sm"
                  variant="ghost"
                  icon={Undo2}
                  aria-label={t.editor.keepOption(draft().name)}
                  title={t.editor.keepOption(draft().name)}
                  onClick={() => change(i, { deleted: false })}
                />
              </Show>
            </div>
          )}
        </For>
        <Button
          size="sm"
          variant="ghost"
          icon={Plus}
          class="self-start"
          onClick={() => setDrafts((all) => [...all, newOption()])}
        >
          {t.editor.addOption}
        </Button>
      </fieldset>
      <RequestError text={failure()} />
      <div class="flex justify-end gap-2">
        <Button variant="ghost" onClick={() => props.onCancel()} disabled={busy()}>
          {t.editor.cancel}
        </Button>
        <Button type="submit" variant="primary" disabled={busy()}>
          {t.editor.save}
        </Button>
      </div>
    </form>
  );
}
