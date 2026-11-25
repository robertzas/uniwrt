import { createSignal, For, Show, untrack } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import { ApiError, errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { Segmented } from "../../ui/Segmented";
import { TextField } from "../../ui/TextField";
import {
  routable,
  subnetOf,
  type TailscaleDraft,
  type TailscaleEdit,
  type TailscaleErrors,
  tailscaleDraft,
  tailscaleErrorsOfIssues,
  tailscaleOf,
  withTailscale,
} from "./tailscale";

const t = strings.vpn.tailscale.editor;

/**
 * Tailscale's editor (network.md, VPN; D-140): on/off, the networks the gateway advertises, the
 * exit node, its machine name, and how it logs in (through the URL, or with an auth key sealed
 * in the vault). Removing it leaves the gateway's daemon as it is.
 */
export function TailscaleEditor(props: { view: SiteView; onClose: () => void }) {
  // The document as the editor opened on it: a save's new head doesn't reset the form.
  const opened = untrack(() => props.view.config);
  const current = opened.vpn.tailscale;
  const isNew = current === null;
  const networks = () => routable(opened.networks);
  const [draft, setDraft] = createSignal<TailscaleDraft>(tailscaleDraft(current, opened));
  const [errors, setErrors] = createSignal<TailscaleErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [removing, setRemoving] = createSignal(false);
  const change = (next: Partial<TailscaleDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };
  const toggle = (id: string, on: boolean) =>
    change({ routes: on ? [...draft().routes, id] : draft().routes.filter((r) => r !== id) });

  const store = async (edit: TailscaleEdit | null, message: string) => {
    setBusy(true);
    setFailure(undefined);
    try {
      // The key's reference comes from the latest document, in case another tab changed it.
      const view = await saveSite(
        (config) =>
          withTailscale(
            config,
            edit && { ...edit.tailscale, auth_key: config.vpn.tailscale?.auth_key ?? null },
          ),
        message,
        edit?.secrets,
      );
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = tailscaleErrorsOfIssues(err.issues.filter((i) => i.severity === "error"));
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
    const { edit, errors: found } = tailscaleOf(draft(), current, opened);
    if (!edit) {
      setErrors(found);
      return;
    }
    void store(edit, isNew ? t.messageNew : t.message);
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={isNew ? t.titleNew : t.title}
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
          <fieldset class="flex flex-col gap-2">
            <legend class="font-medium text-sm">{t.routes}</legend>
            <p class="text-fg-muted text-xs">{t.routesHint}</p>
            <Show
              when={networks().length > 0}
              fallback={<p class="text-fg-muted text-xs">{t.noNetworks}</p>}
            >
              <div class="grid gap-2 sm:grid-cols-2">
                <For each={networks()}>
                  {(network) => (
                    <Checkbox
                      checked={draft().routes.includes(network.id)}
                      onChange={(on) => toggle(network.id, on)}
                      label={network.name}
                      hint={subnetOf(network)}
                    />
                  )}
                </For>
              </div>
            </Show>
            <Show when={errors().routes}>
              <p class="text-crit text-xs">{errors().routes}</p>
            </Show>
          </fieldset>
          <Checkbox
            checked={draft().exitNode}
            onChange={(exitNode) => change({ exitNode })}
            label={t.exitNode}
            hint={t.exitNodeHint}
          />
          <TextField
            label={t.hostname}
            name="ts-hostname"
            value={draft().hostname}
            onInput={(hostname) => change({ hostname })}
            hint={t.hostnameHint}
            error={errors().hostname}
          />
          <div class="flex flex-col gap-2">
            <span class="font-medium text-sm">{t.login}</span>
            <Segmented
              class="self-start"
              label={t.login}
              value={draft().login}
              options={(["url", "key"] as const).map((value) => ({
                value,
                label: t.logins[value],
              }))}
              onChange={(login) => change({ login })}
            />
            <p class="text-fg-muted text-xs">{t.loginHints[draft().login]}</p>
            <Show when={draft().login === "key"}>
              <TextField
                label={t.authKey}
                name="ts-auth-key"
                type="password"
                autocomplete="off"
                value={draft().authKey}
                onInput={(authKey) => change({ authKey })}
                hint={draft().keySaved ? t.authKeySaved : t.authKeyHint}
                error={errors().authKey}
              />
            </Show>
          </div>
          <Checkbox
            checked={draft().enabled}
            onChange={(enabled) => change({ enabled })}
            label={t.enabled}
            hint={t.enabledHint}
          />
          <div class="flex flex-col gap-1 text-fg-muted text-xs">
            <p>{t.package}</p>
            <p>{strings.vpn.tailscale.logs}</p>
          </div>
          <Show when={errors().port}>
            <p class="text-crit text-xs">{errors().port}</p>
          </Show>
          <Show when={failure()}>
            <div class="flex flex-col gap-1">
              <RequestError text={failure()} />
              <For each={other()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
            </div>
          </Show>
          <Show when={removing()}>
            <div
              class="rounded-control border border-crit/40 bg-crit/10 px-3 py-2.5 text-sm"
              role="alert"
            >
              <p>{t.confirmRemove}</p>
              <div class="mt-2 flex gap-2">
                <Button
                  size="sm"
                  variant="primary"
                  disabled={busy()}
                  onClick={() => void store(null, t.messageRemove)}
                >
                  {t.remove}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setRemoving(false)}>
                  {t.cancel}
                </Button>
              </div>
            </div>
          </Show>
          <DialogFooter>
            <Show when={!isNew && !removing()}>
              <Button variant="ghost" onClick={() => setRemoving(true)} disabled={busy()}>
                {t.remove}
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
