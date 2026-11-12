import { TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { SteeringProfile } from "../../api/gen/SteeringProfile";
import { errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { RadioCards } from "../../ui/RadioCards";
import { SavedRevision } from "../../ui/SavedRevision";
import { WpadNote } from "./WpadNote";

const t = strings.wifi.steering;
const PROFILES: readonly SteeringProfile[] = ["off", "conservative", "balanced", "aggressive"];

/**
 * Client steering (wifi.md, Steering & balancing; D-090): the site's usteer profile, each with
 * what it does, saved as one revision. Names the devices without 802.11v, where steering can
 * only disconnect clients (or, Conservative, does nothing).
 */
export function SteeringCard(props: {
  view: SiteView;
  admin: boolean;
  /** The devices whose hostapd lacks 802.11v. */
  basicWpad: readonly { id: number; name: string }[];
}) {
  const [draft, setDraft] = createSignal<SteeringProfile>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const current = () => props.view.config.steering.profile;
  const profile = () => draft() ?? current();
  const changed = () => draft() !== undefined && draft() !== current();
  /** The saved document's steering warnings, or the one the choice would bring. */
  const warnings = () => {
    if (!changed()) {
      return props.view.issues.filter((i) => i.path === "steering").map((i) => i.message);
    }
    const steered = props.view.config.wlans.some((w) => w.enabled && w.roaming.steer);
    return profile() !== "off" && !steered ? [t.noWlan] : [];
  };

  const save = async () => {
    const next = draft();
    if (next === undefined) return;
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, steering: { ...config.steering, profile: next } }),
        t.message(t.profiles[next]?.label ?? next),
      );
      setSaved(view.revision.id);
      setDraft(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <RadioCards
          label={t.title}
          value={profile()}
          options={PROFILES.map((value) => ({
            value,
            label: t.profiles[value]?.label ?? value,
            description: t.profiles[value]?.text ?? "",
          }))}
          disabled={!props.admin || busy()}
          onChange={(value) => {
            setDraft(value);
            setSaved(undefined);
          }}
        />
        <p class="text-fg-subtle text-xs">{t.footnote}</p>
        <For each={warnings()}>
          {(warning) => (
            <p class="flex items-center gap-2 text-warn text-xs">
              <Icon icon={TriangleAlert} class="size-3.5 shrink-0" />
              {warning}
            </p>
          )}
        </For>
        <Show when={current() !== "off" && props.basicWpad.length > 0}>
          <WpadNote text={t.basicWpad(props.basicWpad.length, current())} nodes={props.basicWpad} />
        </Show>
        <Show when={changed()}>
          <div class="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
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
