import { createSignal, For, Show } from "solid-js";
import type { Ddns } from "../../../api/gen/Ddns";
import type { DdnsSource } from "../../../api/gen/DdnsSource";
import type { SiteView } from "../../../api/gen/SiteView";
import { ApiError, errorText } from "../../../lib/api";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { FormField, SELECT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { Segmented } from "../../../ui/Segmented";
import { TextField } from "../../../ui/TextField";
import {
  type DdnsDraft,
  type DdnsErrors,
  ddnsDraft,
  ddnsErrorsOfIssues,
  ddnsOf,
  needsSecret,
  PROVIDERS,
  type ProviderKind,
  withDdns,
  zoneOf,
} from "./ddns";

const t = strings.networks.ddns.editor;
const SOURCES: readonly DdnsSource[] = ["wan", "web"];

/**
 * A DDNS name's editor (network.md, Other services; D-132): the hostname, the provider with the
 * fields it needs and its token as a vault secret, the record and where its address comes from.
 * A save is the site's next revision; the server's issues come back on their fields.
 */
export function DdnsEditor(props: {
  view: SiteView;
  /** The entry to edit; `undefined` adds one. */
  ddns: Ddns | undefined;
  onClose: () => void;
}) {
  const isNew = props.ddns === undefined;
  const [draft, setDraft] = createSignal<DdnsDraft>(ddnsDraft(props.ddns));
  const [errors, setErrors] = createSignal<DdnsErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [deleting, setDeleting] = createSignal(false);
  const list = () => props.view.config.services.ddns;
  const others = () => list().filter((d) => d.id !== props.ddns?.id);
  const index = () => {
    const at = list().findIndex((d) => d.id === props.ddns?.id);
    return at < 0 ? list().length : at;
  };
  const change = (next: Partial<DdnsDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };
  const provider = (kind: ProviderKind) =>
    change({
      provider: kind,
      ...(kind === "cloudflare" && draft().zone === "" && { zone: zoneOf(draft().hostname) }),
    });
  const kept = () => props.ddns?.secret != null && props.ddns.provider.kind === draft().provider;
  const secretLabel = () => t.secret[draft().provider] ?? "";

  const store = async (
    edit: Ddns | { remove: string },
    message: string,
    secrets?: Record<string, string>,
  ) => {
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => withDdns(config, edit), message, secrets);
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = ddnsErrorsOfIssues(
          err.issues.filter((issue) => issue.severity === "error"),
          index(),
        );
        setErrors(mapped.errors);
        setOther(mapped.other);
        setFailure(err.message);
      } else {
        setFailure(errorText(err));
      }
    } finally {
      setBusy(false);
    }
  };

  const save = () => {
    const { ddns, secrets, errors: found } = ddnsOf(draft(), props.ddns, others());
    if (!ddns) {
      setErrors(found);
      return;
    }
    const message = isNew ? t.messageNew(ddns.hostname) : t.message(ddns.hostname);
    void store(ddns, message, secrets);
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={isNew ? t.titleNew : t.title(props.ddns?.hostname ?? "")}
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
            save();
          }}
        >
          <div class="grid gap-3 sm:grid-cols-2">
            <TextField
              label={t.hostname}
              name="ddns-hostname"
              autocomplete="off"
              value={draft().hostname}
              onInput={(hostname) => change({ hostname })}
              hint={draft().provider === "duckdns" ? t.duckdnsHint : t.hostnameHint}
              error={errors().hostname}
            />
            <FormField label={t.provider} for="ddns-provider">
              <select
                id="ddns-provider"
                class={SELECT}
                value={draft().provider}
                onChange={(event) => provider(event.currentTarget.value as ProviderKind)}
              >
                <For each={PROVIDERS}>
                  {(kind) => <option value={kind}>{strings.networks.ddns.providers[kind]}</option>}
                </For>
              </select>
            </FormField>
            <Show when={draft().provider === "cloudflare"}>
              <TextField
                label={t.zone}
                name="ddns-zone"
                value={draft().zone}
                onInput={(zone) => change({ zone })}
                hint={t.zoneHint}
                error={errors().zone}
              />
            </Show>
            <Show when={draft().provider === "custom"}>
              <div class="sm:col-span-2">
                <TextField
                  label={t.url}
                  name="ddns-url"
                  value={draft().url}
                  onInput={(url) => change({ url })}
                  hint={t.urlHint}
                  error={errors().url}
                />
              </div>
            </Show>
            <Show
              when={
                draft().provider === "no_ip" ||
                draft().provider === "dynu" ||
                draft().provider === "custom"
              }
            >
              <TextField
                label={draft().provider === "custom" ? t.usernameOptional : t.username}
                name="ddns-username"
                autocomplete="off"
                value={draft().username}
                onInput={(username) => change({ username })}
                error={errors().username}
              />
            </Show>
            <Show when={needsSecret(draft())}>
              <TextField
                label={secretLabel()}
                name="ddns-secret"
                type="password"
                autocomplete="new-password"
                value={draft().secret}
                onInput={(secret) => change({ secret })}
                hint={kept() ? t.secretKeep : t.secretHint[draft().provider]}
                error={errors().secret}
              />
            </Show>
          </div>
          <div class="grid gap-4 sm:grid-cols-2">
            <div class="flex flex-col gap-2">
              <span class="font-medium text-sm">{t.record}</span>
              <Segmented
                class="self-start"
                label={t.record}
                value={draft().ipv6 ? "aaaa" : "a"}
                options={(["a", "aaaa"] as const).map((value) => ({
                  value,
                  label: t.records[value] ?? value,
                }))}
                onChange={(value) => change({ ipv6: value === "aaaa" })}
              />
            </div>
            <div class="flex flex-col gap-2">
              <span class="font-medium text-sm">{t.source}</span>
              <Segmented
                class="self-start"
                label={t.source}
                value={draft().source}
                options={SOURCES.map((value) => ({ value, label: t.sources[value] ?? value }))}
                onChange={(source) => change({ source })}
              />
              <p class="text-fg-muted text-xs">{t.sourceHints[draft().source]}</p>
            </div>
          </div>
          <Checkbox
            checked={draft().enabled}
            onChange={(enabled) => change({ enabled })}
            label={t.enabled}
            hint={t.enabledHint}
          />
          <Show when={failure()}>
            <div class="flex flex-col gap-1">
              <RequestError text={failure()} />
              <For each={other()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
            </div>
          </Show>
          <Show when={deleting()}>
            <div
              class="rounded-control border border-crit/40 bg-crit/10 px-3 py-2.5 text-sm"
              role="alert"
            >
              <p>{t.confirmDelete(props.ddns?.hostname ?? "")}</p>
              <div class="mt-2 flex gap-2">
                <Button
                  size="sm"
                  variant="primary"
                  disabled={busy()}
                  onClick={() =>
                    props.ddns &&
                    void store({ remove: props.ddns.id }, t.messageDelete(props.ddns.hostname))
                  }
                >
                  {t.delete}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setDeleting(false)}>
                  {t.cancel}
                </Button>
              </div>
            </div>
          </Show>
          <DialogFooter>
            <Show when={!isNew && !deleting()}>
              <Button variant="ghost" onClick={() => setDeleting(true)} disabled={busy()}>
                {t.delete}
              </Button>
            </Show>
            <span class="flex-1" />
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
