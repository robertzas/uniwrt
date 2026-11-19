import { createSignal, For, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { VpnPeer } from "../../api/gen/VpnPeer";
import type { WireGuard } from "../../api/gen/WireGuard";
import { ApiError, errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { FormField, INPUT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { RadioCards } from "../../ui/RadioCards";
import { SavedRevision } from "../../ui/SavedRevision";
import { Segmented } from "../../ui/Segmented";
import { TextField } from "../../ui/TextField";
import {
  type PeerDraft,
  type PeerErrors,
  peerDraft,
  peerErrorsOfIssues,
  peerOf,
  withPeer,
} from "./wireguard";

const t = strings.vpn.wireguard.peer;

/**
 * A peer's editor (network.md, VPN; D-140): its name, a phone or laptop (full or split tunnel)
 * or another site (its networks and where the gateway dials it), its tunnel address (the next
 * free one for a new peer) and, optionally, its own public key. A save is the site's next
 * revision, which makes the keys a new peer lacks; its configuration can be shown right after.
 */
export function PeerEditor(props: {
  view: SiteView;
  /** The view's server. */
  server: WireGuard;
  /** The peer to edit; `undefined` adds one. */
  peer: VpnPeer | undefined;
  /** Shows the saved peer's configuration. */
  onShow: (peer: VpnPeer) => void;
  onClose: () => void;
}) {
  const isNew = props.peer === undefined;
  const server = () => props.server;
  const [draft, setDraft] = createSignal<PeerDraft>(peerDraft(props.peer, server()));
  const [errors, setErrors] = createSignal<PeerErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<{ revision: number; peer: VpnPeer | undefined }>();
  const [deleting, setDeleting] = createSignal(false);
  const change = (next: Partial<PeerDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };

  const store = async (
    edit: VpnPeer | { remove: string },
    message: string,
    show: boolean,
    secrets?: Record<string, null>,
  ) => {
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => withPeer(config, edit), message, secrets);
      const peer =
        "remove" in edit
          ? undefined
          : view.config.vpn.wireguard?.peers.find((p) => p.id === edit.id);
      if (show && peer) props.onShow(peer);
      else setSaved({ revision: view.revision.id, peer });
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const id = "remove" in edit ? edit.remove : edit.id;
        const mapped = peerErrorsOfIssues(
          err.issues.filter((i) => i.severity === "error"),
          id,
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

  const save = (show: boolean) => {
    const {
      peer,
      secrets,
      errors: found,
    } = peerOf(draft(), props.peer, server(), props.view.config);
    if (!peer) {
      setErrors(found);
      return;
    }
    void store(peer, isNew ? t.messageNew(peer.name) : t.message(peer.name), show, secrets);
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={isNew ? t.titleNew : t.title(props.peer?.name ?? "")}
      size="lg"
    >
      <Show
        when={saved() === undefined}
        fallback={
          <div class="flex flex-col gap-4 px-5 pb-5">
            <SavedRevision revision={saved()?.revision ?? 0} />
            <div class="flex justify-end gap-2">
              <Show when={saved()?.peer}>
                {(peer) => (
                  <Button variant="secondary" onClick={() => props.onShow(peer())}>
                    {t.showConfig}
                  </Button>
                )}
              </Show>
              <Button onClick={() => props.onClose()}>{t.close}</Button>
            </div>
          </div>
        }
      >
        <form
          class="flex flex-col gap-5 px-5 pt-1 pb-5"
          onSubmit={(event) => {
            event.preventDefault();
            save(false);
          }}
        >
          <div class="grid gap-3 sm:grid-cols-2">
            <TextField
              label={t.name}
              name="peer-name"
              autocomplete="off"
              value={draft().name}
              onInput={(name) => change({ name })}
              hint={t.nameHint}
              error={errors().name}
            />
            <TextField
              label={t.address}
              name="peer-address"
              value={draft().address}
              onInput={(address) => change({ address })}
              error={errors().address}
            />
          </div>
          <RadioCards
            label={t.kind}
            value={draft().kind}
            options={(["client", "site"] as const).map((value) => ({
              value,
              ...t.kinds[value],
            }))}
            onChange={(kind) => change({ kind })}
          />
          <Show
            when={draft().kind === "site"}
            fallback={
              <div class="flex flex-col gap-2">
                <span class="font-medium text-sm">{t.tunnel}</span>
                <Segmented
                  class="self-start"
                  label={t.tunnel}
                  value={draft().fullTunnel ? "full" : "split"}
                  options={(["full", "split"] as const).map((value) => ({
                    value,
                    label: t.tunnels[value],
                  }))}
                  onChange={(value) => change({ fullTunnel: value === "full" })}
                />
                <p class="text-fg-muted text-xs">
                  {t.tunnelHints[draft().fullTunnel ? "full" : "split"]}
                </p>
              </div>
            }
          >
            <FormField
              label={t.networks}
              for="peer-networks"
              hint={t.networksHint}
              error={errors().networks}
            >
              <textarea
                id="peer-networks"
                class={[INPUT, "h-20 py-2 font-mono"]}
                value={draft().networks}
                onInput={(event) => change({ networks: event.currentTarget.value })}
                aria-invalid={errors().networks ? "true" : undefined}
              />
            </FormField>
            <div class="grid gap-3 sm:grid-cols-[1fr_8rem]">
              <TextField
                label={t.host}
                name="peer-host"
                value={draft().host}
                onInput={(host) => change({ host })}
                hint={t.hostHint}
                error={errors().host}
              />
              <TextField
                label={t.port}
                name="peer-port"
                value={draft().port}
                onInput={(port) => change({ port })}
                error={errors().port}
              />
            </div>
          </Show>
          <div class="flex flex-col gap-3">
            <Checkbox
              checked={draft().ownKey}
              onChange={(ownKey) => change({ ownKey })}
              label={t.ownKey}
              hint={t.ownKeyHint}
            />
            <Show when={draft().ownKey}>
              <TextField
                label={t.publicKey}
                name="peer-public-key"
                autocomplete="off"
                value={draft().publicKey}
                onInput={(publicKey) => change({ publicKey })}
                error={errors().publicKey}
              />
            </Show>
            <Checkbox
              checked={draft().enabled}
              onChange={(enabled) => change({ enabled })}
              label={t.enabled}
              hint={t.enabledHint}
            />
          </div>
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
              <p>{t.confirmDelete(props.peer?.name ?? "")}</p>
              <div class="mt-2 flex gap-2">
                <Button
                  size="sm"
                  variant="primary"
                  disabled={busy()}
                  onClick={() =>
                    props.peer &&
                    void store({ remove: props.peer.id }, t.messageDelete(props.peer.name), false)
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
          <div class="flex flex-wrap items-center gap-2">
            <Show when={!isNew && !deleting()}>
              <Button variant="ghost" onClick={() => setDeleting(true)} disabled={busy()}>
                {t.delete}
              </Button>
            </Show>
            <span class="flex-1" />
            <Button variant="ghost" onClick={() => props.onClose()} disabled={busy()}>
              {t.cancel}
            </Button>
            <Show
              when={isNew}
              fallback={
                <Button type="submit" variant="primary" disabled={busy()}>
                  {t.save}
                </Button>
              }
            >
              <Button variant="secondary" type="submit" disabled={busy()}>
                {t.save}
              </Button>
              <Button variant="primary" disabled={busy()} onClick={() => save(true)}>
                {t.saveShow}
              </Button>
            </Show>
          </div>
        </form>
      </Show>
    </Dialog>
  );
}
