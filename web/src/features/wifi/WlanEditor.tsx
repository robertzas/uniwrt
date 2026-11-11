import { createSignal, For, Show } from "solid-js";
import type { NodeScope } from "../../api/gen/NodeScope";
import type { SiteView } from "../../api/gen/SiteView";
import type { Wlan } from "../../api/gen/Wlan";
import type { WlanSecurity } from "../../api/gen/WlanSecurity";
import { ApiError, errorText } from "../../lib/api";
import { saveSite, sortNetworks } from "../../lib/site";
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
  BANDS,
  type DraftErrors,
  draftOf,
  errorsOfIssues,
  needsPassphrase,
  type Preset,
  presetWlan,
  SECURITIES,
  type WlanDraft,
  withWlan,
  wlanOf,
} from "./draft";
import { ScheduleField, TOGGLE, toggled } from "./ScheduleField";
import { BAND_DOT } from "./WlanRow";

const t = strings.wifi.editor;
const PRESETS: readonly Preset[] = ["main", "guest", "iot"];
const SCOPES: readonly NodeScope["scope"][] = ["all", "only", "except"];
/** The schedule shows once the controller enforces it (M5.1's schedules part, D-083). */
const SCHEDULES = false;

/**
 * A WLAN's editor (wifi.md, WLANs): its name and network, security, bands, the devices that
 * broadcast it, options and schedule. A new one starts from a preset. A save is the site's next
 * revision; the server's issues come back on their fields.
 */
export function WlanEditor(props: {
  view: SiteView;
  /** The WLAN to edit; `undefined` adds one. */
  wlan: Wlan | undefined;
  nodes: readonly { id: number; name: string }[];
  onClose: () => void;
}) {
  const isNew = props.wlan === undefined;
  const [preset, setPreset] = createSignal<Preset>("main");
  const [current, setCurrent] = createSignal<Wlan>(
    props.wlan ?? presetWlan("main", props.view.config),
  );
  const [draft, setDraft] = createSignal<WlanDraft>(draftOf(current()));
  const [errors, setErrors] = createSignal<DraftErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [deleting, setDeleting] = createSignal(false);
  const others = () => props.view.config.wlans.filter((w) => w.id !== props.wlan?.id);
  const networks = () => sortNetworks(props.view.config.networks);
  const change = (next: Partial<WlanDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };
  /** Another preset refills the form but keeps the name and passphrase typed so far. */
  const start = (next: Preset) => {
    const wlan = presetWlan(next, props.view.config);
    setPreset(next);
    setCurrent(wlan);
    setDraft((now) => ({ ...draftOf(wlan), ssid: now.ssid, passphrase: now.passphrase }));
    setErrors({});
  };
  const flip = <T,>(list: readonly T[], item: T): T[] =>
    list.includes(item) ? list.filter((other) => other !== item) : [...list, item];

  const store = async (
    edit: Wlan | { remove: string },
    message: string,
    secrets?: Record<string, string | null>,
  ) => {
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => withWlan(config, edit), message, secrets);
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = errorsOfIssues(
          err.issues.filter((issue) => issue.severity === "error"),
          "remove" in edit ? edit.remove : edit.id,
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
    const { wlan, secrets, errors: found } = wlanOf(draft(), current(), others());
    if (!wlan) {
      setErrors(found);
      return;
    }
    void store(wlan, isNew ? t.messageNew(wlan.ssid) : t.message(wlan.ssid), secrets);
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={isNew ? t.titleNew : t.title(props.wlan?.ssid ?? "")}
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
          <Show when={isNew}>
            <div class="flex flex-wrap items-center gap-3">
              <Segmented
                label={t.preset}
                value={preset()}
                options={PRESETS.map((value) => ({ value, label: t.presets[value] ?? value }))}
                onChange={start}
              />
              <p class="text-fg-muted text-xs">{t.presetHints[preset()]}</p>
            </div>
          </Show>
          <section class="flex flex-col gap-3" aria-label={t.general}>
            <h3 class="font-semibold text-sm">{t.general}</h3>
            <div class="grid gap-3 sm:grid-cols-2">
              <TextField
                label={t.ssid}
                name="ssid"
                autocomplete="off"
                value={draft().ssid}
                onInput={(ssid) => change({ ssid })}
                error={errors().ssid}
              />
              <FormField
                label={t.networkField}
                for="wlan-network"
                hint={t.networkHint}
                error={errors().network}
              >
                <select
                  id="wlan-network"
                  class={SELECT}
                  value={draft().network}
                  onChange={(event) => change({ network: event.currentTarget.value })}
                >
                  <For each={networks()}>
                    {(network) => <option value={network.id}>{network.name}</option>}
                  </For>
                </select>
              </FormField>
            </div>
            <Checkbox
              checked={draft().enabled}
              onChange={(enabled) => change({ enabled })}
              label={t.enabled}
              hint={t.enabledHint}
            />
          </section>
          <section class="flex flex-col gap-3" aria-label={t.securitySection}>
            <h3 class="font-semibold text-sm">{t.securitySection}</h3>
            <div class="grid gap-3 sm:grid-cols-2">
              <FormField
                label={t.mode}
                for="wlan-security"
                hint={strings.wifi.securityHint[draft().security]}
              >
                <select
                  id="wlan-security"
                  class={SELECT}
                  value={draft().security}
                  onChange={(event) =>
                    change({ security: event.currentTarget.value as WlanSecurity })
                  }
                >
                  <For each={SECURITIES}>
                    {(mode) => <option value={mode}>{strings.wifi.security[mode]}</option>}
                  </For>
                </select>
              </FormField>
              <Show when={needsPassphrase(draft().security)}>
                <TextField
                  label={t.passphrase}
                  name="wlan-passphrase"
                  type="password"
                  autocomplete="new-password"
                  value={draft().passphrase}
                  onInput={(passphrase) => change({ passphrase })}
                  hint={current().passphrase === null ? t.passphraseHint : t.passphraseKeep}
                  error={errors().passphrase}
                />
              </Show>
            </div>
          </section>
          <section class="flex flex-col gap-3" aria-label={t.bands}>
            <h3 class="font-semibold text-sm">{t.bands}</h3>
            <div class="flex flex-wrap gap-2">
              <For each={BANDS}>
                {(band) => (
                  <button
                    type="button"
                    aria-pressed={draft().bands.includes(band) ? "true" : "false"}
                    class={[TOGGLE, toggled(draft().bands.includes(band))]}
                    onClick={() => change({ bands: flip(draft().bands, band) })}
                  >
                    <span class={["size-2 rounded-full", BAND_DOT[band]]} aria-hidden="true" />
                    {strings.device.radios.band[band]}
                  </button>
                )}
              </For>
            </div>
            <p class={["text-xs", errors().bands ? "text-crit" : "text-fg-muted"]}>
              {errors().bands ?? t.bandsHint}
            </p>
          </section>
          <section class="flex flex-col gap-3" aria-label={t.devices}>
            <h3 class="font-semibold text-sm">{t.devices}</h3>
            <Segmented
              class="self-start"
              label={t.scope}
              value={draft().scope}
              options={SCOPES.map((value) => ({ value, label: t.scopes[value] ?? value }))}
              onChange={(scope) => change({ scope })}
            />
            <Show when={draft().scope !== "all"}>
              <div class="grid gap-2 sm:grid-cols-2">
                <For
                  each={props.nodes}
                  fallback={<p class="text-fg-muted text-sm">{t.noDevices}</p>}
                >
                  {(node) => (
                    <Checkbox
                      checked={draft().nodes.includes(node.id)}
                      onChange={() => change({ nodes: flip(draft().nodes, node.id) })}
                      label={node.name}
                    />
                  )}
                </For>
              </div>
              <Show when={errors().nodes}>
                <p class="text-crit text-xs">{errors().nodes}</p>
              </Show>
            </Show>
          </section>
          <section class="flex flex-col gap-3" aria-label={t.options}>
            <h3 class="font-semibold text-sm">{t.options}</h3>
            <Checkbox
              checked={draft().isolation}
              onChange={(isolation) => change({ isolation })}
              label={t.isolation}
              hint={t.isolationHint}
            />
            <Checkbox
              checked={draft().hidden}
              onChange={(hidden) => change({ hidden })}
              label={t.hidden}
              hint={t.hiddenHint}
            />
            <div class="grid gap-3 sm:grid-cols-2">
              <TextField
                label={t.maxClients}
                name="wlan-max-clients"
                value={draft().maxClients}
                onInput={(maxClients) => change({ maxClients })}
                hint={t.maxClientsHint}
                error={errors().maxClients}
              />
            </div>
          </section>
          <Show when={SCHEDULES}>
            <ScheduleField
              scheduled={draft().scheduled}
              spans={draft().spans}
              zone={props.view.config.site.timezone}
              error={errors().schedule}
              onChange={change}
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
              <p>{t.confirmDelete(props.wlan?.ssid ?? "")}</p>
              <div class="mt-2 flex gap-2">
                <Button
                  size="sm"
                  variant="primary"
                  disabled={busy()}
                  onClick={() =>
                    props.wlan &&
                    void store({ remove: props.wlan.id }, t.messageDelete(props.wlan.ssid))
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
            <Button type="submit" variant="primary" disabled={busy()}>
              {t.save}
            </Button>
          </div>
        </form>
      </Show>
    </Dialog>
  );
}
