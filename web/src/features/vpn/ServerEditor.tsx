import { createSignal, For, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { WireGuard } from "../../api/gen/WireGuard";
import { ApiError, errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { FormField, SELECT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { Segmented } from "../../ui/Segmented";
import { TextField } from "../../ui/TextField";
import {
  type ServerDraft,
  type ServerErrors,
  serverDraft,
  serverErrorsOfIssues,
  serverOf,
  withServer,
} from "./wireguard";

const t = strings.vpn.wireguard.server;

/**
 * The WireGuard server's editor (network.md, VPN; D-140): on/off, the UDP port, the gateway's
 * tunnel address, what peers dial (a DDNS name or a host) and whether they use the gateway's
 * DNS. The first save makes the server's keypair. Removing it deletes every peer.
 */
export function ServerEditor(props: { view: SiteView; onClose: () => void }) {
  const current = props.view.config.vpn.wireguard;
  const isNew = current === null;
  const ddns = () => props.view.config.services.ddns;
  const [draft, setDraft] = createSignal<ServerDraft>(serverDraft(current, ddns()));
  const [errors, setErrors] = createSignal<ServerErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [removing, setRemoving] = createSignal(false);
  const change = (next: Partial<ServerDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };

  const store = async (server: WireGuard | null, message: string) => {
    setBusy(true);
    setFailure(undefined);
    try {
      // The peers and keys come from the latest document, in case another tab changed them.
      const view = await saveSite(
        (config) =>
          withServer(
            config,
            server && {
              ...server,
              private_key: config.vpn.wireguard?.private_key ?? null,
              public_key: config.vpn.wireguard?.public_key ?? null,
              peers: config.vpn.wireguard?.peers ?? [],
            },
          ),
        message,
      );
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = serverErrorsOfIssues(err.issues.filter((i) => i.severity === "error"));
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
    const { server, errors: found } = serverOf(draft(), current, props.view.config);
    if (!server) {
      setErrors(found);
      return;
    }
    void store(server, isNew ? t.messageNew : t.message);
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
            <div class="flex justify-end">
              <Button onClick={() => props.onClose()}>{t.close}</Button>
            </div>
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
              label={t.port}
              name="wg-port"
              value={draft().port}
              onInput={(port) => change({ port })}
              hint={t.portHint}
              error={errors().port}
            />
            <TextField
              label={t.address}
              name="wg-address"
              value={draft().address}
              onInput={(address) => change({ address })}
              hint={t.addressHint}
              error={errors().address}
            />
          </div>
          <div class="flex flex-col gap-2">
            <span class="font-medium text-sm">{t.endpoint}</span>
            <Segmented
              class="self-start"
              label={t.endpoint}
              value={draft().endpoint}
              options={(["ddns", "host"] as const).map((value) => ({
                value,
                label: t.endpoints[value],
              }))}
              onChange={(endpoint) => change({ endpoint })}
            />
            <Show
              when={draft().endpoint === "ddns"}
              fallback={
                <TextField
                  label={t.host}
                  name="wg-host"
                  value={draft().host}
                  onInput={(host) => change({ host })}
                  hint={t.hostHint}
                  error={errors().endpoint}
                />
              }
            >
              <Show
                when={ddns().length > 0}
                fallback={<p class="text-crit text-xs">{errors().endpoint ?? t.noDdns}</p>}
              >
                <FormField label={t.ddns} for="wg-ddns" error={errors().endpoint}>
                  <select
                    id="wg-ddns"
                    class={SELECT}
                    value={draft().ddns}
                    onChange={(event) => change({ ddns: event.currentTarget.value })}
                  >
                    <For each={ddns()}>
                      {(entry) => <option value={entry.id}>{entry.hostname}</option>}
                    </For>
                  </select>
                </FormField>
              </Show>
            </Show>
          </div>
          <Checkbox
            checked={draft().dns}
            onChange={(dns) => change({ dns })}
            label={t.dns}
            hint={t.dnsHint}
          />
          <Checkbox
            checked={draft().enabled}
            onChange={(enabled) => change({ enabled })}
            label={t.enabled}
            hint={t.enabledHint}
          />
          <Show when={isNew}>
            <p class="text-fg-muted text-xs">{t.keysNote}</p>
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
          <div class="flex flex-wrap items-center gap-2">
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
          </div>
        </form>
      </Show>
    </Dialog>
  );
}
