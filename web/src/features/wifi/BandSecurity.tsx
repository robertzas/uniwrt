import { createSignal, For, Show } from "solid-js";
import type { Band } from "../../api/gen/Band";
import type { WlanSecurity } from "../../api/gen/WlanSecurity";
import { strings } from "../../lib/strings";
import { FormField, SELECT } from "../../ui/FormField";
import { type BandSecurity as Overrides, SECURITIES, SIX_GHZ } from "./draft";

const t = strings.wifi.editor;

/**
 * Each chosen band's own security mode (wifi.md, WLANs; D-157), such as WPA2 on 2.4 GHz for
 * devices that can't join WPA3. Hidden behind a link until a band has one, and only with two
 * bands or more: one band changes the WLAN's mode instead. 6 GHz offers the modes it allows.
 */
export function BandSecurity(props: {
  bands: readonly Band[];
  security: WlanSecurity;
  value: Overrides;
  onChange: (value: Overrides) => void;
}) {
  const [shown, setShown] = createSignal(false);
  const chosen = () => props.bands.filter((band) => props.value[band] !== undefined);
  const open = () => shown() || chosen().length > 0;
  const set = (band: Band, mode: string) => {
    const next = { ...props.value };
    if (mode === "" || mode === props.security) delete next[band];
    else next[band] = mode as WlanSecurity;
    props.onChange(next);
  };
  return (
    <Show when={props.bands.length > 1 || chosen().length > 0}>
      <Show
        when={open()}
        fallback={
          <button
            type="button"
            class="self-start text-accent text-xs hover:underline"
            onClick={() => setShown(true)}
          >
            {t.bandSecurityShow}
          </button>
        }
      >
        <div class="flex flex-col gap-2">
          <div class="grid gap-3 sm:grid-cols-2">
            <For each={props.bands}>
              {(band) => (
                <FormField
                  label={t.bandSecurityLabel(strings.device.radios.band[band])}
                  for={`wlan-security-${band}`}
                >
                  <select
                    id={`wlan-security-${band}`}
                    class={SELECT}
                    value={props.value[band] ?? ""}
                    onChange={(event) => set(band, event.currentTarget.value)}
                  >
                    <option value="">
                      {t.bandSecuritySame(strings.wifi.security[props.security] ?? props.security)}
                    </option>
                    <For
                      each={SECURITIES.filter(
                        (mode) =>
                          mode !== props.security && (band !== "6g" || SIX_GHZ.includes(mode)),
                      )}
                    >
                      {(mode) => <option value={mode}>{strings.wifi.security[mode]}</option>}
                    </For>
                  </select>
                </FormField>
              )}
            </For>
          </div>
          <p class="text-fg-muted text-xs">{t.bandSecurityHint}</p>
        </div>
      </Show>
    </Show>
  );
}
