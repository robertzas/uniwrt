import { CircleAlert, CircleCheck, Send } from "lucide";
import { createSignal, Match, Show, Switch, untrack } from "solid-js";
import type { NodeSummary } from "../../../api/gen/NodeSummary";
import type { NotifierView } from "../../../api/gen/NotifierView";
import type { Placeholder } from "../../../api/gen/Placeholder";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { Segmented } from "../../../ui/Segmented";
import {
  checkDraft,
  type Draft,
  type DraftErrors,
  draftOf,
  KINDS,
  type Kind,
  MAX_NAME,
} from "./draft";
import { FilterFields } from "./FilterFields";
import { EmailFields, Input, NtfyFields } from "./fields";
import { WebhookFields } from "./WebhookFields";

const t = strings.settings.notifications.editor;

/** How the test went; `why` is the server's word when it failed. */
type Outcome = { ok: boolean; why: string };

/**
 * Adds a notifier or edits one: its kind's fields (the kind is chosen once), its filter, and a
 * test that sends what is in the form, with the stored secrets for those not retyped. Checked as
 * the server checks it; the server's message shows when it refuses anyway.
 */
export function NotifierEditor(props: {
  /** The notifier to edit; undefined for a new one. */
  notifier: NotifierView | undefined;
  /** A new one's first kind. */
  kind?: Kind;
  nodes: readonly NodeSummary[];
  placeholders: readonly Placeholder[];
  linkUrl: string | null;
  onClose: () => void;
  onSaved: (notifier: NotifierView) => void;
}) {
  // The dialog mounts per notifier, so its draft starts from the one it was opened for.
  const [draft, setDraft] = createSignal<Draft>(untrack(() => draftOf(props.notifier, props.kind)));
  const [errors, setErrors] = createSignal<DraftErrors>({});
  const [failure, setFailure] = createSignal<string | undefined>();
  const [saving, setSaving] = createSignal(false);
  const [testing, setTesting] = createSignal(false);
  const [tested, setTested] = createSignal<Outcome | undefined>();
  const change = (next: Partial<Draft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setTested(undefined);
  };
  const stored = () => ({
    token: props.notifier?.secrets.token ?? false,
    password: props.notifier?.secrets.password ?? false,
  });

  const checked = () => {
    const result = checkDraft(draft(), props.placeholders);
    setErrors("errors" in result ? result.errors : {});
    return "input" in result ? result.input : undefined;
  };

  const save = (event: SubmitEvent) => {
    event.preventDefault();
    const input = checked();
    if (input === undefined) return;
    setFailure(undefined);
    setSaving(true);
    const notifier = props.notifier;
    (notifier === undefined
      ? api.notifiers.create(input)
      : api.notifiers.update(notifier.id, input)
    )
      .then((saved) => props.onSaved(saved))
      .catch((error: unknown) => setFailure(errorText(error) ?? t.saveFailed))
      .finally(() => setSaving(false));
  };

  const test = () => {
    const input = checked();
    if (input === undefined) return;
    setTested(undefined);
    setTesting(true);
    api.notifiers
      .test({ ...input, id: props.notifier?.id ?? null })
      .then(() => setTested({ ok: true, why: t.testSent }))
      .catch((error: unknown) => setTested({ ok: false, why: errorText(error) ?? t.testFailed }))
      .finally(() => setTesting(false));
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={props.notifier === undefined ? t.create : t.edit(props.notifier.name)}
      description={t.description}
      size="lg"
    >
      <form onSubmit={save} novalidate>
        <div class="flex max-h-[min(40rem,62vh)] flex-col gap-5 overflow-y-auto px-5 pt-2 pb-5">
          <Show when={props.notifier === undefined}>
            <div class="flex flex-col gap-1.5">
              <span class="font-medium text-fg text-sm">{t.kind}</span>
              <Segmented
                label={t.kind}
                value={draft().kind}
                options={KINDS.map((kind) => ({
                  value: kind,
                  label: strings.settings.notifications.kinds[kind],
                }))}
                onChange={(kind: Kind) => change({ kind })}
                class="self-start"
              />
              <p class="text-fg-subtle text-xs">{t.kindHint[draft().kind]}</p>
            </div>
          </Show>
          <div class="flex flex-wrap items-start justify-between gap-4">
            <Input
              label={t.name}
              value={draft().name}
              onInput={(name) => change({ name })}
              error={errors().name}
              placeholder={t.namePlaceholder[draft().kind]}
              hint={t.nameHint(MAX_NAME)}
              class="min-w-56 flex-1"
            />
            <Checkbox
              label={t.enabled}
              hint={draft().enabled ? undefined : t.enabledHint}
              checked={draft().enabled}
              onChange={(enabled) => change({ enabled })}
              class="max-w-56 sm:mt-7"
            />
          </div>
          <Switch>
            <Match when={draft().kind === "ntfy"}>
              <NtfyFields draft={draft()} errors={errors()} stored={stored()} onChange={change} />
            </Match>
            <Match when={draft().kind === "webhook"}>
              <WebhookFields
                draft={draft()}
                errors={errors()}
                storedUrl={props.notifier?.secrets.url ?? false}
                placeholders={props.placeholders}
                linkUrl={props.linkUrl}
                onChange={change}
              />
            </Match>
            <Match when={draft().kind === "email"}>
              <EmailFields draft={draft()} errors={errors()} stored={stored()} onChange={change} />
            </Match>
          </Switch>
          <FilterFields draft={draft()} errors={errors()} nodes={props.nodes} onChange={change} />
        </div>
        <div class="flex flex-col gap-2 border-edge border-t px-5 py-3">
          <Show when={tested()}>
            {(outcome) => (
              <p
                role="status"
                class="flex min-w-0 items-start gap-1.5 text-sm"
                title={outcome().why}
              >
                <Icon
                  icon={outcome().ok ? CircleCheck : CircleAlert}
                  class={["mt-0.5 size-4 shrink-0", outcome().ok ? "text-ok" : "text-crit"]}
                />
                <span class="line-clamp-3 min-w-0 break-words">{outcome().why}</span>
              </p>
            )}
          </Show>
          <Show when={failure()}>
            {(message) => (
              <p role="alert" class="flex min-w-0 items-start gap-1.5 text-sm" title={message()}>
                <Icon icon={CircleAlert} class="mt-0.5 size-4 shrink-0 text-crit" />
                <span class="line-clamp-3 min-w-0 break-words">{message()}</span>
              </p>
            )}
          </Show>
          <div class="flex flex-wrap items-center justify-end gap-2">
            <Button
              type="button"
              icon={Send}
              class="mr-auto"
              disabled={testing() || saving()}
              onClick={test}
            >
              {testing() ? `${t.testing}…` : t.test}
            </Button>
            <Button type="button" variant="ghost" onClick={() => props.onClose()}>
              {t.cancel}
            </Button>
            <Button type="submit" variant="primary" disabled={saving()}>
              {saving() ? `${t.saving}…` : t.save}
            </Button>
          </div>
        </div>
      </form>
    </Dialog>
  );
}
