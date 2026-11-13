import { Info, TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import { errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { Segmented } from "../../ui/Segmented";
import { TextField } from "../../ui/TextField";
import {
  MESH_BANDS,
  type MeshDraft,
  type MeshErrors,
  meshChanged,
  meshCut,
  meshDraftOf,
  meshErrors,
  meshSave,
  ROUTINGS,
  randomMeshKey,
} from "./mesh";

const t = strings.wifi.mesh;

/** A line of the card's notes: information, or a warning. */
function Note(props: { warn?: boolean; text: string }) {
  return (
    <p class={["flex items-start gap-2 text-xs", props.warn ? "text-warn" : "text-fg-muted"]}>
      <Icon icon={props.warn ? TriangleAlert : Info} class="mt-px size-3.5 shrink-0" />
      {props.text}
    </p>
  );
}

/**
 * The mesh's settings (features/mesh.md, Mesh settings; D-100): on or off, its 802.11s ID and
 * key, band and routing algorithm, and whether batman-adv takes over the cables between
 * managed devices on a VLAN of its own (D-094), saved as one revision. A change that cuts the
 * mesh's links says so; the review names the devices it cuts off, and refuses to strand one.
 */
export function MeshCard(props: { view: SiteView; admin: boolean }) {
  const [draft, setDraft] = createSignal<MeshDraft>();
  const [errors, setErrors] = createSignal<MeshErrors>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const mesh = () => props.view.config.mesh;
  const shown = () => draft() ?? meshDraftOf(mesh());
  const changed = () => {
    const now = draft();
    return now !== undefined && meshChanged(now, mesh());
  };
  const cut = () => meshCut(shown(), mesh());
  const locked = () => !props.admin || busy();
  const change = (next: Partial<MeshDraft>) => {
    setDraft({ ...shown(), ...next });
    setErrors({});
    setSaved(undefined);
  };
  /** The saved document's mesh issues, while nothing is edited. */
  const issues = () =>
    changed()
      ? []
      : props.view.issues.filter((i) => i.path.startsWith("mesh")).map((i) => i.message);

  const save = async () => {
    const next = draft();
    if (next === undefined) return;
    const invalid = meshErrors(next, props.view.config.networks);
    if (Object.keys(invalid).length > 0) {
      setErrors(invalid);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const { mesh: edited, secrets, message } = meshSave(next, mesh());
      const view = await saveSite(
        (config) => ({ ...config, mesh: { ...edited, key: config.mesh.key } }),
        message,
        secrets,
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
      <div class="flex flex-col gap-4 px-4 pb-4">
        <Checkbox
          label={t.enabled}
          hint={t.enabledHint}
          checked={shown().enabled}
          disabled={locked()}
          onChange={(enabled) => change({ enabled })}
        />
        <Show when={!shown().enabled}>
          <Note text={t.off} />
        </Show>
        <div class="grid gap-3 sm:grid-cols-2">
          <TextField
            label={t.meshId}
            name="mesh-id"
            value={shown().meshId}
            hint={t.meshIdHint}
            error={errors().meshId}
            disabled={locked()}
            onInput={(meshId) => change({ meshId })}
          />
          <div class="flex items-start gap-2">
            <div class="min-w-0 flex-1">
              <TextField
                label={t.key}
                name="mesh-key"
                type="password"
                autocomplete="new-password"
                value={shown().key}
                hint={mesh().key === null ? t.keyHint : t.keyKeep}
                error={errors().key}
                disabled={locked()}
                onInput={(key) => change({ key })}
              />
            </div>
            <Button
              class="mt-6.5"
              variant="ghost"
              aria-label={t.generateLabel}
              disabled={locked()}
              onClick={() => change({ key: randomMeshKey() })}
            >
              {t.generate}
            </Button>
          </div>
        </div>
        <div class="grid gap-3 sm:grid-cols-2">
          <div class="flex flex-col gap-1.5">
            <span class="font-medium text-fg text-sm">{t.band}</span>
            <Segmented
              class="self-start"
              label={t.band}
              value={shown().band}
              options={MESH_BANDS.map((value) => ({ value, label: t.bands[value] }))}
              disabled={locked()}
              onChange={(band) => change({ band })}
            />
            <p class="text-fg-muted text-xs">{t.bandHints[shown().band]}</p>
          </div>
          <div class="flex flex-col gap-1.5">
            <span class="font-medium text-fg text-sm">{t.routing}</span>
            <Segmented
              class="self-start"
              label={t.routing}
              value={shown().routing}
              options={ROUTINGS.map((value) => ({ value, label: t.routings[value] }))}
              disabled={locked()}
              onChange={(routing) => change({ routing })}
            />
            <p class="text-fg-muted text-xs">{t.routingHints[shown().routing]}</p>
          </div>
        </div>
        <Checkbox
          label={t.wired}
          hint={t.wiredHint}
          checked={shown().wired}
          disabled={locked()}
          onChange={(wired) => change({ wired })}
        />
        <Show when={shown().wired}>
          <div class="max-w-sm pl-6.5">
            <TextField
              label={t.vlan}
              value={shown().vlan}
              hint={t.vlanHint}
              error={errors().vlan}
              disabled={locked()}
              onInput={(vlan) => change({ vlan })}
            />
          </div>
        </Show>
        <Show when={changed() && cut()}>
          {(kind) => <Note warn text={kind() === "off" ? t.cuts : t.moves} />}
        </Show>
        <For each={issues()}>{(issue) => <Note warn text={issue} />}</For>
        <Show when={changed()}>
          <div class="flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setDraft(undefined);
                setErrors({});
              }}
              disabled={busy()}
            >
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
