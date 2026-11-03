import { CircleAlert } from "lucide";
import { createSignal, createUniqueId, For, Show } from "solid-js";
import type { EventLevel } from "../../api/gen/EventLevel";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { RuleView } from "../../api/gen/RuleView";
import { api, errorText } from "../../lib/api";
import { LEVELS } from "../../lib/events";
import { formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { FormField, SELECT } from "../../ui/FormField";
import { Icon } from "../../ui/Icon";
import { Segmented } from "../../ui/Segmented";
import { TextField } from "../../ui/TextField";
import { EventFields } from "./EventFields";
import { MetricFields } from "./MetricFields";
import {
  COOLDOWN_CHOICES,
  checkDraft,
  type Draft,
  type DraftErrors,
  draftOf,
  withChoice,
} from "./rules";

const t = strings.alerts.editor;

/**
 * Creates a rule or edits one (built in or the user's): a metric beyond a threshold, or events
 * counted within a window. Checked as the server checks it; the server's message shows when it
 * refuses anyway.
 */
export function RuleEditor(props: {
  /** The rule to edit; undefined for a new one. */
  rule: RuleView | undefined;
  nodes: readonly NodeSummary[];
  patterns: readonly { id: string; meaning: string }[];
  onClose: () => void;
  onSaved: (rule: RuleView) => void;
}) {
  const id = createUniqueId();
  // The dialog mounts per rule, so its draft starts from the rule it was opened for.
  const [draft, setDraft] = createSignal<Draft>(draftOf(props.rule));
  const [errors, setErrors] = createSignal<DraftErrors>({});
  const [failure, setFailure] = createSignal<string | undefined>();
  const [saving, setSaving] = createSignal(false);
  const change = (next: Partial<Draft>) => setDraft((now) => ({ ...now, ...next }));

  const save = (event: SubmitEvent) => {
    event.preventDefault();
    const checked = checkDraft(draft());
    if ("errors" in checked) {
      setErrors(checked.errors);
      return;
    }
    setErrors({});
    setFailure(undefined);
    setSaving(true);
    const rule = props.rule;
    (rule === undefined
      ? api.alertRules.create(checked.spec)
      : api.alertRules.update(rule.id, checked.spec)
    )
      .then((saved) => props.onSaved(saved))
      .catch((error: unknown) => setFailure(errorText(error) ?? strings.alerts.rules.changeFailed))
      .finally(() => setSaving(false));
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={props.rule === undefined ? t.create : t.edit(props.rule.name)}
      description={t.description}
      size="lg"
    >
      <form onSubmit={save} novalidate>
        <div class="flex max-h-[min(36rem,68vh)] flex-col gap-5 overflow-y-auto px-5 pt-2 pb-5">
          <TextField
            label={t.name}
            value={draft().name}
            onInput={(name) => change({ name })}
            error={errors().name}
            autofocus={props.rule === undefined}
          />
          <div class="flex flex-wrap items-end justify-between gap-4">
            <div class="flex flex-col gap-1.5">
              <span class="font-medium text-fg text-sm">{t.level}</span>
              <Segmented
                label={t.level}
                value={draft().level}
                options={LEVELS.map((level) => ({
                  value: level,
                  label: strings.events.levelName[level],
                }))}
                onChange={(level: EventLevel) => change({ level })}
              />
            </div>
            <Checkbox
              label={t.enabled}
              hint={draft().enabled ? undefined : t.enabledHint}
              checked={draft().enabled}
              onChange={(enabled) => change({ enabled })}
              class="max-w-64"
            />
          </div>
          <div class="flex flex-col gap-1.5">
            <span class="font-medium text-fg text-sm">{t.type}</span>
            <Segmented
              label={t.type}
              value={draft().type}
              options={(["metric", "event"] as const).map((type) => ({
                value: type,
                label: t.types[type],
              }))}
              onChange={(type: Draft["type"]) => change({ type })}
              class="self-start"
            />
            <p class="text-fg-subtle text-xs">{t.typeHint[draft().type]}</p>
          </div>
          <Show
            when={draft().type === "metric"}
            fallback={
              <EventFields
                draft={draft()}
                errors={errors()}
                nodes={props.nodes}
                patterns={props.patterns}
                onChange={change}
              />
            }
          >
            <MetricFields draft={draft()} errors={errors()} nodes={props.nodes} onChange={change} />
          </Show>
          <FormField
            label={t.cooldown}
            for={`${id}-cooldown`}
            hint={t.cooldownHint}
            class="sm:max-w-xs"
          >
            <select
              id={`${id}-cooldown`}
              class={SELECT}
              value={String(draft().cooldown)}
              onChange={(event) => change({ cooldown: Number(event.currentTarget.value) })}
            >
              <For each={withChoice(COOLDOWN_CHOICES, draft().cooldown)}>
                {(s) => <option value={String(s)}>{s === 0 ? t.none : formatDuration(s)}</option>}
              </For>
            </select>
          </FormField>
        </div>
        <div class="flex flex-wrap items-center justify-end gap-2 border-edge border-t px-5 py-3">
          <Show when={failure()}>
            {(message) => (
              <p role="alert" class="mr-auto flex items-start gap-1.5 text-sm">
                <Icon icon={CircleAlert} class="mt-0.5 size-4 shrink-0 text-crit" />
                {message()}
              </p>
            )}
          </Show>
          <Button type="button" variant="ghost" onClick={() => props.onClose()}>
            {t.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={saving()}>
            {saving() ? `${t.saving}…` : t.save}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
