import { createMemo, createSignal, createUniqueId, For, Show } from "solid-js";
import type { Band } from "../../api/gen/Band";
import { countries } from "../../lib/countries";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { FormField, SELECT } from "../../ui/FormField";
import { TextField } from "../../ui/TextField";
import type { WifiDraft, WifiErrors } from "./plan";
import { Note } from "./RouterStep";

const t = strings.setup.wifi;

/** `2.4 GHz and 5 GHz`. */
export function bandNames(bands: readonly Band[]): string {
  return bands.map((band) => strings.device.radios.band[band]).join(" and ");
}

/**
 * The wizard's Wi-Fi step (D-080: minimal): one SSID and passphrase on the router's 2.4 and
 * 5 GHz radios, in the main network, and the country the radios need. A router without radios
 * skips it.
 */
export function WifiStep(props: {
  draft: WifiDraft;
  errors: WifiErrors;
  /** The bands the WLAN goes on; none when the router has no such radio. */
  bands: readonly Band[];
  /** The router has a 6 GHz radio, which stays off. */
  sixGhz: boolean;
  onChange: (next: Partial<WifiDraft>) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const list = createMemo(countries);
  const countryId = createUniqueId();
  // What is wrong shows once Continue was tried, not on an empty form.
  const [tried, setTried] = createSignal(false);
  const errors = (): WifiErrors => (tried() ? props.errors : {});
  const next = () => {
    if (Object.keys(props.errors).length === 0) props.onNext();
    else setTried(true);
  };
  return (
    <div class="flex flex-col gap-5">
      <div>
        <h2 class="font-semibold text-lg tracking-tight">{t.heading}</h2>
        <Show when={props.bands.length > 0}>
          <p class="mt-1.5 text-fg-muted text-sm">{t.intro}</p>
        </Show>
      </div>
      <Show
        when={props.bands.length > 0}
        fallback={<Note tone="info" title={t.none} text={t.noneHint} />}
      >
        <Checkbox
          checked={props.draft.enabled}
          onChange={(enabled) => props.onChange({ enabled })}
          label={t.enable}
          hint={props.draft.enabled ? t.bands(bandNames(props.bands)) : t.off}
        />
        <Show when={props.draft.enabled}>
          <div class="grid gap-4 sm:grid-cols-2">
            <TextField
              label={t.ssid}
              name="ssid"
              autocomplete="off"
              value={props.draft.ssid}
              onInput={(ssid) => props.onChange({ ssid })}
              error={errors().ssid}
            />
            <TextField
              label={t.passphrase}
              name="wifi-passphrase"
              type="password"
              autocomplete="new-password"
              value={props.draft.passphrase}
              onInput={(passphrase) => props.onChange({ passphrase })}
              hint={t.passphraseHint}
              error={errors().passphrase}
            />
            <FormField
              label={t.country}
              for={countryId}
              hint={t.countryHint}
              error={errors().country}
              class="sm:col-span-2"
            >
              <select
                id={countryId}
                value={props.draft.country}
                onChange={(event) => props.onChange({ country: event.currentTarget.value })}
                class={[SELECT, { "border-crit": errors().country !== undefined }]}
              >
                <option value="">{t.pickCountry}</option>
                <For each={list()}>{(c) => <option value={c.code}>{c.name}</option>}</For>
              </select>
            </FormField>
          </div>
          <Show when={props.sixGhz}>
            <p class="text-fg-muted text-sm">{t.sixGhz}</p>
          </Show>
        </Show>
      </Show>
      <div class="flex flex-wrap justify-end gap-2">
        <Button variant="ghost" onClick={props.onBack}>
          {strings.setup.back}
        </Button>
        <Button variant="primary" onClick={next}>
          {strings.setup.next}
        </Button>
      </div>
    </div>
  );
}
