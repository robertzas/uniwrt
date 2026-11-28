import { createSignal, createUniqueId, For, Show } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { RadioFacts } from "../../../api/gen/RadioFacts";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import { errorText } from "../../../lib/api";
import { fixedChannel, fixedPower } from "../../../lib/radio";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { FormField, SELECT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import {
  carriesMesh,
  channelChoices,
  maxPowerOf,
  overrideFrom,
  overrideOf,
  phyBandOf,
  policyOf,
  type RadioDraft,
  radioDraft,
  widthChoices,
  withRadio,
} from "./radios";

const r = strings.device.radios;
const t = r.dialog;

/**
 * Fixes one radio's channel, width and power by hand, or turns it off (wifi.md, RF planner:
 * pins; D-155): the node's radio override, which render and the RF planner keep, saved as the
 * site's next revision. Each choice can follow the band's setting on the Wi-Fi page again.
 */
export function RadioDialog(props: {
  node: Node;
  facts: NodeFacts | null;
  radio: RadioFacts;
  config: SiteConfig;
  onClose: () => void;
  onSaved: (revision: number) => void;
}) {
  const ids = { channel: createUniqueId(), width: createUniqueId(), power: createUniqueId() };
  const stored = () => overrideOf(props.config, props.node.id, props.radio.name);
  const [draft, setDraft] = createSignal<RadioDraft>(radioDraft(stored()));
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const phy = () => phyBandOf(props.facts, props.radio);
  const policy = () => policyOf(props.config, props.radio.band);
  const max = () => maxPowerOf(phy());
  const keep = () => (typeof draft().channel === "number" ? (draft().channel as number) : null);
  const channels = () => channelChoices(phy(), props.radio.band, keep());
  const widths = () => widthChoices(phy(), props.radio.band);
  const managed = () =>
    props.config.nodes.find((n) => n.node === props.node.id)?.managed.includes("wireless") ?? false;
  const bandChannel = () => {
    const band = policy();
    const fixed = band ? fixedChannel(band.channel) : null;
    return fixed === null ? t.auto : r.fixedChannel(fixed);
  };
  const bandPower = () => {
    const band = policy();
    const fixed = band ? fixedPower(band.tx_power) : null;
    return fixed === null ? t.auto : r.fixedPower(fixed);
  };
  /** 1 dBm up to the most, and a power fixed already above it. */
  const powers = () => {
    const all = Array.from({ length: max() }, (_, i) => i + 1);
    const now = draft().power;
    return typeof now === "number" && now > max() ? [...all, now] : all;
  };
  const change = (next: Partial<RadioDraft>) => setDraft((was) => ({ ...was, ...next }));

  const save = async () => {
    const override = overrideFrom(props.radio.name, draft());
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => withRadio(config, props.node.id, props.radio.name, override),
        override
          ? t.message(props.node.name, props.radio.name)
          : t.messageSite(props.node.name, props.radio.name),
      );
      props.onSaved(view.revision.id);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && props.onClose()}
      title={t.title(r.band[props.radio.band], props.radio.name)}
      description={t.description}
    >
      <form
        class="flex flex-col gap-4 px-5 pt-2 pb-5"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <Show when={props.node.mode !== "managed"}>
          <p class="text-fg-muted text-sm">{t.observed}</p>
        </Show>
        <Show when={props.node.mode === "managed" && !managed()}>
          <p class="text-warn text-sm">{t.unmanaged}</p>
        </Show>
        <Checkbox
          label={t.enabled}
          hint={t.enabledHint}
          checked={draft().enabled}
          onChange={(enabled) => change({ enabled })}
        />
        <Show when={draft().enabled}>
          <FormField
            label={t.channel}
            for={ids.channel}
            hint={
              carriesMesh(props.radio)
                ? t.mesh
                : channels().some((c) => c.dfs)
                  ? t.channelHint
                  : undefined
            }
          >
            <select
              id={ids.channel}
              class={SELECT}
              value={String(draft().channel)}
              onChange={(event) => {
                const value = event.currentTarget.value;
                change({ channel: value === "site" || value === "auto" ? value : Number(value) });
              }}
            >
              <option value="site">{t.site(bandChannel())}</option>
              <option value="auto">{t.channelAuto}</option>
              <For each={channels()}>
                {(c) => (
                  <option value={String(c.number)}>
                    {t.channelOption(c.number, c.dfs, c.radar)}
                  </option>
                )}
              </For>
            </select>
          </FormField>
          <FormField label={t.width} for={ids.width}>
            <select
              id={ids.width}
              class={SELECT}
              value={String(draft().width)}
              onChange={(event) => {
                const value = event.currentTarget.value;
                change({
                  width: value === "site" ? "site" : (Number(value) as RadioDraft["width"]),
                });
              }}
            >
              <option value="site">{t.widthSite(policy()?.width ?? 20)}</option>
              <For each={widths()}>
                {(w) => <option value={String(w)}>{t.widthOption(w)}</option>}
              </For>
            </select>
          </FormField>
          <FormField label={t.power} for={ids.power} hint={t.dbmHint(max())}>
            <select
              id={ids.power}
              class={SELECT}
              value={String(draft().power)}
              onChange={(event) => {
                const value = event.currentTarget.value;
                change({ power: value === "site" || value === "auto" ? value : Number(value) });
              }}
            >
              <option value="site">{t.site(bandPower())}</option>
              <option value="auto">{t.powerAuto}</option>
              <For each={powers()}>
                {(dbm) => <option value={String(dbm)}>{r.fixedPower(dbm)}</option>}
              </For>
            </select>
          </FormField>
        </Show>
        <RequestError text={failure()} />
        <DialogFooter>
          <Button variant="ghost" onClick={props.onClose} disabled={busy()}>
            {t.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {t.save}
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}
