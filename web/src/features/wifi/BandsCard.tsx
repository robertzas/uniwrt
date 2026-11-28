import { createSignal, For, Show } from "solid-js";
import type { ChannelWidth } from "../../api/gen/ChannelWidth";
import type { SiteView } from "../../api/gen/SiteView";
import { errorText } from "../../lib/api";
import { BAND_CHANNELS, BAND_WIDTHS, isDfs, MAX_DBM } from "../../lib/radio";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { SELECT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { type BandDraft, bandDrafts, bandsChanged, withBands } from "./bands";

const t = strings.wifi.bands;
const bandName = strings.device.radios.band;
const POWERS = Array.from({ length: MAX_DBM }, (_, i) => i + 1);

const channelText = (n: number | null) => (n === null ? t.auto : t.fixedChannel(n));
const powerText = (dbm: number | null) => (dbm === null ? t.auto : t.dbm(dbm));

/** A select's value: `"auto"` or a number. */
const parse = (value: string) => (value === "auto" ? null : Number(value));

/**
 * The radios' defaults per band (wifi.md, Radios; D-155): channel and power Auto or fixed, and
 * the width, on every device's radios of the band that don't set their own (the device page's
 * Radios card does). Saved as the site's next revision; the RF planner keeps what is fixed.
 */
export function BandsCard(props: { view: SiteView; admin: boolean }) {
  const [drafts, setDrafts] = createSignal<BandDraft[]>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const rows = () => drafts() ?? bandDrafts(props.view.config);
  const update = (band: string, next: Partial<BandDraft>) =>
    setDrafts((now) => now?.map((d) => (d.band === band ? { ...d, ...next } : d)));
  const fixesChannel = () => rows().some((d) => d.channel !== null);

  const save = async () => {
    const now = drafts();
    if (!now) return;
    if (!bandsChanged(props.view.config, now)) {
      setDrafts(undefined);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => withBands(config, now), t.message);
      setSaved(view.revision.id);
      setDrafts(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && !drafts()}>
            <Button
              size="sm"
              onClick={() => {
                setDrafts(bandDrafts(props.view.config));
                setSaved(undefined);
                setFailure(undefined);
              }}
            >
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <div class="overflow-x-auto">
          <table class="w-full text-sm">
            <thead class="text-left text-fg-muted text-xs">
              <tr>
                <th class="py-1.5 pr-3 font-medium">{t.band}</th>
                <th class="py-1.5 pr-3 font-medium">{t.channel}</th>
                <th class="py-1.5 pr-3 font-medium">{t.width}</th>
                <th class="py-1.5 font-medium">{t.power}</th>
              </tr>
            </thead>
            <tbody class="divide-y divide-edge">
              <For each={rows()} keyed={(row) => row.band}>
                {(row) => (
                  <tr aria-label={bandName[row().band]}>
                    <td class="py-2 pr-3 font-medium leading-9">{bandName[row().band]}</td>
                    <Show
                      when={drafts()}
                      fallback={
                        <>
                          <td class="py-2 pr-3">{channelText(row().channel)}</td>
                          <td class="py-2 pr-3">{t.mhz(row().width)}</td>
                          <td class="py-2">{powerText(row().power)}</td>
                        </>
                      }
                    >
                      <td class="py-2 pr-3">
                        <select
                          aria-label={t.label(t.channel, bandName[row().band])}
                          class={[SELECT, "max-w-40"]}
                          value={String(row().channel ?? "auto")}
                          onChange={(event) =>
                            update(row().band, { channel: parse(event.currentTarget.value) })
                          }
                        >
                          <option value="auto">{t.auto}</option>
                          <For each={BAND_CHANNELS[row().band]}>
                            {(n) => (
                              <option value={String(n)}>
                                {t.channelOption(n, isDfs(row().band, n))}
                              </option>
                            )}
                          </For>
                        </select>
                      </td>
                      <td class="py-2 pr-3">
                        <select
                          aria-label={t.label(t.width, bandName[row().band])}
                          class={[SELECT, "max-w-32"]}
                          value={String(row().width)}
                          onChange={(event) =>
                            update(row().band, {
                              width: Number(event.currentTarget.value) as ChannelWidth,
                            })
                          }
                        >
                          <For each={BAND_WIDTHS[row().band]}>
                            {(w) => <option value={String(w)}>{t.mhz(w)}</option>}
                          </For>
                        </select>
                      </td>
                      <td class="py-2">
                        <select
                          aria-label={t.label(t.power, bandName[row().band])}
                          class={[SELECT, "max-w-32"]}
                          value={String(row().power ?? "auto")}
                          onChange={(event) =>
                            update(row().band, { power: parse(event.currentTarget.value) })
                          }
                        >
                          <option value="auto">{t.auto}</option>
                          <For each={POWERS}>
                            {(dbm) => <option value={String(dbm)}>{t.dbm(dbm)}</option>}
                          </For>
                        </select>
                      </td>
                    </Show>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
        <p class="text-fg-muted text-xs">{t.widthHint}</p>
        <Show when={drafts() && fixesChannel()}>
          <p class="text-warn text-xs">{t.channelHint}</p>
        </Show>
        <Show when={drafts()}>
          <div class="flex flex-wrap justify-end gap-2">
            <Button variant="ghost" onClick={() => setDrafts(undefined)} disabled={busy()}>
              {t.cancel}
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={busy()}>
              {t.save}
            </Button>
          </div>
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
    </Card>
  );
}
