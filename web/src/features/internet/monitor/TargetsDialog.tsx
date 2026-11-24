import { Plus, Trash2 } from "lucide";
import { createSignal, createUniqueId, For, Show } from "solid-js";
import type { WanMonitorView } from "../../../api/gen/WanMonitorView";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { FormField, SELECT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import {
  checkTargets,
  draftOf,
  newDraft,
  PROBE_KINDS,
  type ProbeKind,
  TARGETS_MAX,
  type TargetDraft,
  type TargetErrors,
  VIA_KINDS,
  type ViaKind,
} from "./targets";

const t = strings.internet.monitor;
const ed = t.editor;

/**
 * The monitor's targets (admins; D-144): on or off, and up to 12 pings, lookups and fetches.
 * Saving checks them as the server does, and the server runs a round at once.
 */
export function TargetsDialog(props: {
  view: WanMonitorView;
  onClose: () => void;
  onSaved: (view: WanMonitorView) => void;
}) {
  const [enabled, setEnabled] = createSignal(props.view.settings.enabled);
  const [drafts, setDrafts] = createSignal<TargetDraft[]>(props.view.settings.targets.map(draftOf));
  const [errors, setErrors] = createSignal<TargetErrors[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);

  const change = (i: number, next: Partial<TargetDraft>) => {
    setDrafts((all) => all.map((draft, j) => (j === i ? { ...draft, ...next } : draft)));
    setErrors((all) => all.map((found, j) => (j === i ? {} : found)));
  };
  const remove = (i: number) => {
    setDrafts((all) => all.filter((_, j) => j !== i));
    setErrors((all) => all.filter((_, j) => j !== i));
  };
  const save = async () => {
    const checked = checkTargets(enabled(), drafts());
    if ("errors" in checked) {
      setErrors(checked.errors);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      props.onSaved(await api.wan.saveMonitor(checked.settings));
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && props.onClose()}
      title={ed.title}
      description={ed.description}
      size="lg"
    >
      <form
        class="flex flex-col gap-4 px-5 pb-5"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <Checkbox checked={enabled()} onChange={setEnabled} label={ed.enabled} />
        <ol class="flex flex-col gap-3">
          <For each={drafts()} keyed={false}>
            {(draft, i) => (
              <TargetFields
                draft={draft()}
                errors={errors()[i] ?? {}}
                onChange={(next) => change(i, next)}
                onRemove={() => remove(i)}
              />
            )}
          </For>
        </ol>
        <div class="flex flex-wrap items-center gap-3">
          <Button
            size="sm"
            icon={Plus}
            disabled={drafts().length >= TARGETS_MAX}
            onClick={() => setDrafts((all) => [...all, newDraft()])}
          >
            {ed.add}
          </Button>
          <Show when={drafts().length >= TARGETS_MAX}>
            <span class="text-fg-subtle text-xs">{ed.full(TARGETS_MAX)}</span>
          </Show>
        </div>
        <RequestError text={failure()} />
        <div class="flex justify-end gap-2">
          <Button variant="ghost" onClick={props.onClose} disabled={busy()}>
            {ed.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {ed.save}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function TargetFields(props: {
  draft: TargetDraft;
  errors: TargetErrors;
  onChange: (next: Partial<TargetDraft>) => void;
  onRemove: () => void;
}) {
  const id = createUniqueId();
  const d = () => props.draft;
  return (
    <li class="grid items-start gap-3 rounded-control border border-edge p-3 sm:grid-cols-[minmax(0,1fr)_7rem_minmax(0,1.4fr)_auto]">
      <TextField
        label={ed.name}
        value={d().name}
        onInput={(name) => props.onChange({ name })}
        error={props.errors.name}
        hint={d().id === null ? undefined : ed.series(d().id ?? "")}
      />
      <FormField label={ed.kind} for={`${id}-kind`}>
        <select
          id={`${id}-kind`}
          class={SELECT}
          value={d().kind}
          onChange={(event) => props.onChange({ kind: event.currentTarget.value as ProbeKind })}
        >
          <For each={PROBE_KINDS}>{(kind) => <option value={kind}>{t.kinds[kind]}</option>}</For>
        </select>
      </FormField>
      <Show when={d().kind === "icmp"}>
        <TextField
          label={ed.host}
          value={d().host}
          onInput={(host) => props.onChange({ host })}
          error={props.errors.host}
          placeholder={ed.hostPlaceholder}
        />
      </Show>
      <Show when={d().kind === "dns"}>
        <TextField
          label={ed.lookup}
          value={d().lookup}
          onInput={(lookup) => props.onChange({ lookup })}
          error={props.errors.lookup}
        />
      </Show>
      <Show when={d().kind === "http"}>
        <TextField
          label={ed.url}
          value={d().url}
          onInput={(url) => props.onChange({ url })}
          error={props.errors.url}
          placeholder={ed.urlPlaceholder}
        />
      </Show>
      <Button
        variant="ghost"
        icon={Trash2}
        class="sm:mt-7"
        aria-label={ed.remove(d().name.trim())}
        onClick={props.onRemove}
      />
      <Show when={d().kind === "dns"}>
        <div
          class={[
            "grid items-start gap-3 sm:col-span-3 sm:col-start-2",
            { "sm:grid-cols-2": d().via === "server" },
          ]}
        >
          <FormField label={ed.via} for={`${id}-via`}>
            <select
              id={`${id}-via`}
              class={SELECT}
              value={d().via}
              onChange={(event) => props.onChange({ via: event.currentTarget.value as ViaKind })}
            >
              <For each={VIA_KINDS}>{(via) => <option value={via}>{ed.vias[via]}</option>}</For>
            </select>
          </FormField>
          <Show when={d().via === "server"}>
            <TextField
              label={ed.server}
              value={d().server}
              onInput={(server) => props.onChange({ server })}
              error={props.errors.server}
            />
          </Show>
        </div>
      </Show>
    </li>
  );
}
