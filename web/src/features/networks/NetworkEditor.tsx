import { createMemo, createSignal, For, Show } from "solid-js";
import type { Network } from "../../api/gen/Network";
import type { Purpose } from "../../api/gen/Purpose";
import type { SiteView } from "../../api/gen/SiteView";
import { ApiError, errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { FormField, INPUT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { TextField } from "../../ui/TextField";
import {
  type DraftErrors,
  draftOf,
  errorsOfIssues,
  type NetworkDraft,
  networkOf,
  usesOf,
  withNetwork,
} from "./draft";
import { Ipv4Fields } from "./Ipv4Fields";
import { ReservationsField } from "./ReservationsField";

const t = strings.networks.editor;
const PURPOSES: Purpose[] = ["iot", "guest", "mgmt", "custom"];

/**
 * A network's editor (network.md, Networks): general, IPv4 and DHCP with its options and PXE,
 * IPv6, behaviour and reservations. A save is the site's next revision; the server's issues come
 * back on their fields.
 */
export function NetworkEditor(props: {
  view: SiteView;
  network: Network;
  isNew: boolean;
  nodeName: (id: number) => string;
  onClose: () => void;
}) {
  const [draft, setDraft] = createSignal<NetworkDraft>(draftOf(props.network));
  const [errors, setErrors] = createSignal<DraftErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [deleting, setDeleting] = createSignal(false);
  const others = () => props.view.config.networks.filter((n) => n.id !== props.network.id);
  const isDefault = () => props.network.purpose === "default";
  const uses = createMemo(() =>
    props.isNew ? [] : usesOf(props.view.config, props.network.id, props.nodeName),
  );
  const change = (next: Partial<NetworkDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };

  const store = async (edit: Network | { remove: string }, message: string) => {
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => withNetwork(config, edit), message);
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = errorsOfIssues(
          err.issues.filter((issue) => issue.severity === "error"),
          draft().id.trim(),
          draft().reservations,
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
    const mesh = props.view.config.mesh;
    const backhaul = mesh.enabled ? mesh.wired_vlan : undefined;
    const { network, errors: found } = networkOf(draft(), others(), props.isNew, backhaul);
    if (!network) {
      setErrors(found);
      return;
    }
    void store(
      network,
      props.isNew ? strings.networks.editor.messageNew(network.name) : t.message(network.name),
    );
  };

  const close = (open: boolean) => !open && !busy() && props.onClose();

  return (
    <Dialog
      open
      onOpenChange={close}
      title={props.isNew ? t.titleNew : t.title(props.network.name)}
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
          class="flex flex-col gap-6 px-5 pt-1 pb-5"
          onSubmit={(event) => {
            event.preventDefault();
            save();
          }}
        >
          <section class="flex flex-col gap-3" aria-label={t.general}>
            <h3 class="font-semibold text-sm">{t.general}</h3>
            <div class="grid gap-3 sm:grid-cols-2">
              <TextField
                label={t.name}
                value={draft().name}
                onInput={(name) => change({ name })}
                error={errors().name}
              />
              <Show
                when={props.isNew}
                fallback={
                  <FormField label={t.id} hint={t.idHint}>
                    <p class="flex h-9 items-center font-mono text-fg-muted text-sm">
                      {props.network.id}
                    </p>
                  </FormField>
                }
              >
                <TextField
                  label={t.id}
                  value={draft().id}
                  onInput={(id) => change({ id })}
                  hint={t.idHint}
                  error={errors().id}
                />
              </Show>
              <Show when={!isDefault()}>
                <FormField label={t.purpose} for="network-purpose">
                  <select
                    id="network-purpose"
                    class={[INPUT, "border-edge"]}
                    value={draft().purpose}
                    onChange={(event) => change({ purpose: event.currentTarget.value as Purpose })}
                  >
                    <For each={PURPOSES}>
                      {(purpose) => (
                        <option value={purpose}>{strings.networks.purpose[purpose]}</option>
                      )}
                    </For>
                  </select>
                </FormField>
              </Show>
              <TextField
                label={t.vlan}
                value={draft().vlan}
                onInput={(vlan) => change({ vlan })}
                hint={t.vlanHint}
                error={errors().vlan}
              />
            </div>
          </section>
          <Ipv4Fields
            draft={draft()}
            errors={errors()}
            domain={props.view.config.site.domain}
            change={change}
          />
          <section class="flex flex-col gap-3" aria-label={t.ipv6}>
            <h3 class="font-semibold text-sm">{t.ipv6}</h3>
            <div class="grid gap-3 sm:grid-cols-2">
              <FormField
                label={t.ipv6Mode}
                for="network-ipv6"
                error={errors().ipv6}
                hint={props.view.config.addressing.ula === null ? t.noUla : undefined}
              >
                <select
                  id="network-ipv6"
                  class={[INPUT, "border-edge"]}
                  value={draft().ipv6}
                  onChange={(event) =>
                    change({ ipv6: event.currentTarget.value as NetworkDraft["ipv6"] })
                  }
                >
                  <For each={["off", "ula", "delegated", "both"] as const}>
                    {(mode) => (
                      <option
                        value={mode}
                        disabled={
                          (mode === "ula" || mode === "both") &&
                          props.view.config.addressing.ula === null &&
                          draft().ipv6 !== mode
                        }
                      >
                        {t.ipv6Modes[mode]}
                      </option>
                    )}
                  </For>
                </select>
              </FormField>
              <Show when={draft().ipv6 !== "off"}>
                <FormField
                  label={t.ipv6Clients}
                  for="network-ipv6-clients"
                  error={errors().ipv6Clients}
                >
                  <select
                    id="network-ipv6-clients"
                    class={[INPUT, "border-edge"]}
                    value={draft().ipv6Clients}
                    onChange={(event) =>
                      change({
                        ipv6Clients: event.currentTarget.value as NetworkDraft["ipv6Clients"],
                      })
                    }
                  >
                    <For each={["slaac", "slaac_stateless", "slaac_stateful", "stateful"] as const}>
                      {(mode) => <option value={mode}>{t.ipv6ClientModes[mode]}</option>}
                    </For>
                  </select>
                </FormField>
              </Show>
            </div>
          </section>
          <section class="flex flex-col gap-3" aria-label={t.behaviour}>
            <h3 class="font-semibold text-sm">{t.behaviour}</h3>
            <Checkbox
              checked={draft().isolation}
              onChange={(isolation) => change({ isolation })}
              label={t.isolation}
              hint={t.isolationHint}
            />
            <Checkbox
              checked={draft().mdns}
              onChange={(mdns) => change({ mdns })}
              label={t.mdns}
              hint={t.mdnsHint}
            />
            <Checkbox
              checked={draft().igmp}
              onChange={(igmp) => change({ igmp })}
              label={t.igmp}
              hint={t.igmpHint}
            />
          </section>
          <Show when={draft().ipv4 && draft().dhcp}>
            <ReservationsField
              reservations={draft().reservations}
              errors={errors()}
              onChange={(reservations) => change({ reservations })}
            />
          </Show>
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
              <Show when={uses().length === 0} fallback={<p>{t.used(uses().join(", "))}</p>}>
                <p>{t.confirmDelete(props.network.name)}</p>
                <div class="mt-2 flex gap-2">
                  <Button
                    size="sm"
                    variant="primary"
                    disabled={busy()}
                    onClick={() =>
                      void store({ remove: props.network.id }, t.messageDelete(props.network.name))
                    }
                  >
                    {t.delete}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setDeleting(false)}>
                    {t.cancel}
                  </Button>
                </div>
              </Show>
            </div>
          </Show>
          <div class="flex flex-wrap items-center gap-2">
            <Show when={!props.isNew && !isDefault() && !deleting()}>
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
          </div>
        </form>
      </Show>
    </Dialog>
  );
}
