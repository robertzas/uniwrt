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
import { TextField } from "../../ui/TextField";
import { wiredVlanError } from "./summary";

const t = strings.wifi.mesh;

/** The card's edit: whether the mesh runs over cables, and the VLAN it takes there. */
type Draft = { wired: boolean; vlan: string };

/**
 * The mesh backhaul's settings (features/mesh.md, Wired backhaul; D-094): whether batman-adv
 * takes over the cables between managed devices, and the VLAN it runs on there, saved as one
 * revision. Ports opt out on their device's Ports tab.
 */
export function MeshCard(props: { view: SiteView; admin: boolean }) {
  const [draft, setDraft] = createSignal<Draft>();
  const [error, setError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const mesh = () => props.view.config.mesh;
  const current = (): Draft => ({ wired: mesh().wired, vlan: String(mesh().wired_vlan) });
  const shown = () => draft() ?? current();
  const changed = () => {
    const now = draft();
    return now !== undefined && (now.wired !== mesh().wired || now.vlan.trim() !== current().vlan);
  };
  const change = (next: Partial<Draft>) => {
    setDraft({ ...shown(), ...next });
    setError(undefined);
    setSaved(undefined);
  };
  /** The saved document's wired backhaul issues, while nothing is edited. */
  const issues = () =>
    changed()
      ? []
      : props.view.issues.filter((i) => i.path.startsWith("mesh/wired")).map((i) => i.message);

  const save = async () => {
    const next = draft();
    if (next === undefined) return;
    const invalid = next.wired ? wiredVlanError(next.vlan, props.view.config.networks) : undefined;
    if (invalid) {
      setError(invalid);
      return;
    }
    // Turning wired links off leaves the VLAN as saved.
    const vlan = next.wired ? Number(next.vlan.trim()) : undefined;
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({
          ...config,
          mesh: { ...config.mesh, wired: next.wired, wired_vlan: vlan ?? config.mesh.wired_vlan },
        }),
        t.message(next.wired, vlan ?? mesh().wired_vlan),
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
        <Show when={!mesh().enabled}>
          <p class="flex items-start gap-2 text-fg-muted text-xs">
            <Icon icon={Info} class="mt-px size-3.5 shrink-0" />
            {t.off}
          </p>
        </Show>
        <Checkbox
          label={t.wired}
          hint={t.wiredHint}
          checked={shown().wired}
          disabled={!props.admin || busy()}
          onChange={(wired) => change({ wired })}
        />
        <Show when={shown().wired}>
          <div class="max-w-sm pl-6.5">
            <TextField
              label={t.vlan}
              value={shown().vlan}
              hint={t.vlanHint}
              error={error()}
              disabled={!props.admin || busy()}
              onInput={(vlan) => change({ vlan })}
            />
          </div>
        </Show>
        <For each={issues()}>
          {(issue) => (
            <p class="flex items-center gap-2 text-warn text-xs">
              <Icon icon={TriangleAlert} class="size-3.5 shrink-0" />
              {issue}
            </p>
          )}
        </For>
        <Show when={changed()}>
          <div class="flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setDraft(undefined);
                setError(undefined);
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
