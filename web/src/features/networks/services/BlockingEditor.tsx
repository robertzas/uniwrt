import { Plus, Trash2, X } from "lucide";
import { createMemo, createSignal, createUniqueId, For, Show, untrack } from "solid-js";
import type { BlocklistsView } from "../../../api/gen/BlocklistsView";
import type { Client } from "../../../api/gen/Client";
import type { ListCategory } from "../../../api/gen/ListCategory";
import type { SiteView } from "../../../api/gen/SiteView";
import { ApiError, errorText } from "../../../lib/api";
import { clientName } from "../../../lib/clients";
import { formatCount } from "../../../lib/format";
import { normalizeMac } from "../../../lib/names";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { FormField, INPUT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { TextField } from "../../../ui/TextField";
import {
  type BlockingDraft,
  type BlockingErrors,
  blockingDraft,
  blockingErrorsOfIssues,
  blockingOf,
  estimate,
  listsOf,
  MAX_BYPASS,
  MAX_CUSTOM,
  NO_ERRORS,
  withEnabled,
} from "./blocking";

const t = strings.networks.blocking.editor;
const CATEGORIES: readonly ListCategory[] = ["ads", "malware"];

/**
 * DNS filtering's settings (network.md, Blocklists; D-133): on or off, the catalog's lists with
 * their size against what the gateway holds, custom lists, allowed and blocked domains, the
 * clients that bypass the filter and query statistics. A save is the site's next revision; the
 * server's issues come back on their fields.
 */
export function BlockingEditor(props: {
  view: SiteView;
  lists: BlocklistsView;
  clients: readonly Client[];
  onClose: () => void;
  onSaved: () => void;
}) {
  // The settings as the dialog opened: later revisions don't reset what is being typed.
  const [draft, setDraft] = createSignal<BlockingDraft>(
    untrack(() => blockingDraft(props.view.config.services.blocking)),
  );
  const [errors, setErrors] = createSignal<BlockingErrors>(NO_ERRORS);
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const change = (next: (now: BlockingDraft) => Partial<BlockingDraft>) => {
    setDraft((now) => ({ ...now, ...next(now) }));
    setErrors(NO_ERRORS);
    setOther([]);
  };
  const toggleList = (id: string, on: boolean) =>
    change((now) => ({ lists: on ? [...now.lists, id] : now.lists.filter((l) => l !== id) }));
  const setCustom = (index: number, next: Partial<{ name: string; url: string }>) =>
    change((now) => ({ custom: now.custom.map((c, i) => (i === index ? { ...c, ...next } : c)) }));

  const save = async () => {
    const { blocking, errors: found } = blockingOf(draft());
    if (!blocking) {
      setErrors(found);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, services: { ...config.services, blocking } }),
        t.message(blocking.enabled),
      );
      setSaved(view.revision.id);
      props.onSaved();
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = blockingErrorsOfIssues(err.issues.filter((i) => i.severity === "error"));
        setErrors(mapped.errors);
        setOther(mapped.other);
      }
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={t.title}
      size="lg"
    >
      <Show
        when={saved() === undefined}
        fallback={
          <div class="flex flex-col gap-4 px-5 pb-5">
            <SavedRevision revision={saved() ?? 0} />
            <DialogFooter>
              <Button onClick={() => props.onClose()}>{t.close}</Button>
            </DialogFooter>
          </div>
        }
      >
        <form
          class="flex flex-col gap-5 px-5 pt-1 pb-5"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <Checkbox
            checked={draft().enabled}
            onChange={(on) => setDraft((now) => withEnabled(now, on))}
            label={t.enabled}
            hint={t.enabledHint}
          />
          <Lists draft={draft()} lists={props.lists} onToggle={toggleList} />
          <fieldset class="flex flex-col gap-2">
            <legend class="font-medium text-sm">{t.custom}</legend>
            <p class="text-fg-subtle text-xs">{t.customHint}</p>
            <For each={draft().custom} keyed={false}>
              {(list, i) => (
                <div class="flex items-start gap-1">
                  <div class="grid min-w-0 flex-1 gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
                    <TextField
                      label={t.customName(i + 1)}
                      value={list().name}
                      onInput={(name) => setCustom(i, { name })}
                      error={errors().custom[i]?.name}
                    />
                    <TextField
                      label={t.customUrl(i + 1)}
                      value={list().url}
                      onInput={(url) => setCustom(i, { url })}
                      error={errors().custom[i]?.url}
                    />
                  </div>
                  <Button
                    class="mt-7"
                    size="sm"
                    variant="ghost"
                    icon={Trash2}
                    aria-label={t.removeCustom(i + 1)}
                    onClick={() =>
                      change((now) => ({ custom: now.custom.filter((_, at) => at !== i) }))
                    }
                  />
                </div>
              )}
            </For>
            <Show when={errors().customCount}>
              {(text) => <p class="text-crit text-xs">{text()}</p>}
            </Show>
            <Show when={draft().custom.length < MAX_CUSTOM}>
              <Button
                class="self-start"
                size="sm"
                variant="ghost"
                icon={Plus}
                onClick={() =>
                  change((now) => ({ custom: [...now.custom, { name: "", url: "" }] }))
                }
              >
                {t.addCustom}
              </Button>
            </Show>
          </fieldset>
          <div class="grid gap-3 sm:grid-cols-2">
            <Domains
              label={t.allow}
              hint={t.allowHint}
              value={draft().allow}
              error={errors().allow}
              onInput={(allow) => change(() => ({ allow }))}
            />
            <Domains
              label={t.deny}
              hint={t.denyHint}
              value={draft().deny}
              error={errors().deny}
              onInput={(deny) => change(() => ({ deny }))}
            />
          </div>
          <Bypass
            macs={draft().bypass}
            clients={props.clients}
            error={errors().bypass}
            onChange={(bypass) => change(() => ({ bypass }))}
          />
          <Checkbox
            checked={draft().stats}
            onChange={(stats) => change(() => ({ stats }))}
            label={t.stats}
            hint={t.statsHint}
          />
          <Show when={other().length > 0}>
            <div role="alert" class="flex flex-col gap-1">
              <For each={other()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
            </div>
          </Show>
          <RequestError text={failure()} />
          <DialogFooter>
            <Button variant="ghost" onClick={() => props.onClose()} disabled={busy()}>
              {t.cancel}
            </Button>
            <Button type="submit" variant="primary" disabled={busy()}>
              {t.save}
            </Button>
          </DialogFooter>
        </form>
      </Show>
    </Dialog>
  );
}

/** The catalog's lists by category, and the chosen ones' size against the gateway's capacity. */
function Lists(props: {
  draft: BlockingDraft;
  lists: BlocklistsView;
  onToggle: (id: string, on: boolean) => void;
}) {
  const chosen = () => estimate(props.lists.catalog, props.draft.lists);
  const capacity = () => props.lists.compiled?.capacity;
  const note = () => {
    const cap = capacity();
    return cap !== undefined && chosen() > cap
      ? t.estimateOver(formatCount(chosen()), formatCount(cap))
      : t.estimate(formatCount(chosen()));
  };
  return (
    <fieldset class="flex flex-col gap-3">
      <legend class="font-medium text-sm">{t.lists}</legend>
      <div class="grid gap-4 sm:grid-cols-2">
        <For each={CATEGORIES}>
          {(category) => (
            <div class="flex flex-col gap-2">
              <p class="font-medium text-fg-muted text-xs uppercase tracking-wide">
                {strings.networks.blocking.categories[category]}
              </p>
              <For each={listsOf(props.lists.catalog, category)}>
                {(list) => (
                  <Checkbox
                    checked={props.draft.lists.includes(list.id)}
                    onChange={(on) => props.onToggle(list.id, on)}
                    label={list.name}
                    hint={t.listSize(formatCount(list.domains))}
                  />
                )}
              </For>
            </div>
          )}
        </For>
      </div>
      <p
        class={[
          "text-xs",
          capacity() !== undefined && chosen() > (capacity() ?? 0) ? "text-warn" : "text-fg-subtle",
        ]}
      >
        {note()}
      </p>
    </fieldset>
  );
}

function Domains(props: {
  label: string;
  hint: string;
  value: string;
  error: string | undefined;
  onInput: (value: string) => void;
}) {
  const id = createUniqueId();
  return (
    <FormField label={props.label} for={id} hint={props.hint} error={props.error}>
      <textarea
        id={id}
        rows={5}
        spellcheck={false}
        autocapitalize="off"
        class={[INPUT, "h-auto py-2 font-mono", props.error ? "border-crit" : "border-edge"]}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
      />
    </FormField>
  );
}

/** The bypassing clients, and a field that adds one by name or MAC. */
function Bypass(props: {
  macs: string[];
  clients: readonly Client[];
  error: string | undefined;
  onChange: (macs: string[]) => void;
}) {
  const id = createUniqueId();
  const [text, setText] = createSignal("");
  const [invalid, setInvalid] = createSignal(false);
  const byMac = createMemo(() => new Map(props.clients.map((c) => [c.mac, c])));
  const nameOf = (mac: string) => {
    const client = byMac().get(mac);
    return client ? clientName(client) : mac;
  };
  const offered = () =>
    props.clients
      .filter((c) => !props.macs.includes(c.mac))
      .sort((a, b) => clientName(a).localeCompare(clientName(b)));
  const add = () => {
    const typed = text().trim();
    const named = props.clients.find(
      (c) => clientName(c).toLowerCase() === typed.toLowerCase(),
    )?.mac;
    const mac = normalizeMac(typed) ?? named;
    if (mac === undefined || (Number.parseInt(mac.slice(0, 2), 16) & 1) === 1) {
      setInvalid(true);
      return;
    }
    if (!props.macs.includes(mac)) props.onChange([...props.macs, mac]);
    setText("");
    setInvalid(false);
  };
  return (
    <fieldset class="flex flex-col gap-2">
      <legend class="font-medium text-sm">{t.bypass}</legend>
      <p class="text-fg-subtle text-xs">{t.bypassHint}</p>
      <Show
        when={props.macs.length > 0}
        fallback={<p class="text-fg-muted text-sm">{t.bypassNone}</p>}
      >
        <ul class="flex flex-wrap gap-2">
          <For each={props.macs}>
            {(mac) => (
              <li class="flex items-center gap-1 rounded-control border border-edge py-0.5 pr-0.5 pl-2.5 text-sm">
                <span>{nameOf(mac)}</span>
                <Show when={nameOf(mac) !== mac}>
                  <span class="font-mono text-fg-subtle text-xs">{mac}</span>
                </Show>
                <Button
                  size="sm"
                  variant="ghost"
                  icon={X}
                  aria-label={t.bypassRemove(nameOf(mac))}
                  onClick={() => props.onChange(props.macs.filter((m) => m !== mac))}
                />
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show when={props.macs.length < MAX_BYPASS}>
        <FormField
          label={t.bypassAdd}
          for={id}
          error={invalid() ? t.errors.mac : props.error}
          class="max-w-md"
        >
          <div class="flex gap-2">
            <input
              id={id}
              list={`${id}-clients`}
              autocomplete="off"
              placeholder={t.bypassPlaceholder}
              class={[INPUT, invalid() ? "border-crit" : "border-edge"]}
              value={text()}
              onInput={(event) => {
                setText(event.currentTarget.value);
                setInvalid(false);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  add();
                }
              }}
            />
            <datalist id={`${id}-clients`}>
              <For each={offered()}>
                {(client) => <option value={client.mac} label={clientName(client)} />}
              </For>
            </datalist>
            <Button variant="secondary" icon={Plus} onClick={add} disabled={text().trim() === ""}>
              {t.bypassAdd}
            </Button>
          </div>
        </FormField>
      </Show>
    </fieldset>
  );
}
