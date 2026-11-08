import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { createEffect, createSignal, For, Show, useContext } from "solid-js";
import type { ManagedPackage } from "../../../api/gen/ManagedPackage";
import type { Node } from "../../../api/gen/Node";
import { api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { showToast } from "../../../ui/Toast";
import { changeOf, GATEWAY_ONLY, managedOf, PACKAGES, RENDERED, withManaged } from "./packages";

const p = strings.device.packages;

/**
 * The UCI packages the node lets uniwrt own (D-011), saved as the site's next revision; the site
 * is read again whenever the latest revision changes.
 */
export function PackagesCard(props: { node: Node; admin: boolean }) {
  const live = useContext(LiveContext);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
    // The previous revision's choice stays while the next one loads.
    placeholderData: keepPreviousData,
  }));
  const saved = () => (site.isSuccess ? managedOf(site.data.config, props.node.id) : []);
  const [chosen, setChosen] = createSignal<readonly ManagedPackage[]>([]);
  const [saving, setSaving] = createSignal(false);
  createEffect(saved, (packages) => {
    setChosen(packages);
  });
  const change = () => changeOf(saved(), chosen());
  const changed = () => change().added.length > 0 || change().removed.length > 0;

  /** Why `name` can't be chosen here, if it can't: render doesn't write it (on this node). */
  const unavailable = (name: ManagedPackage) =>
    !RENDERED.includes(name)
      ? p.later
      : GATEWAY_ONLY.includes(name) && props.node.role !== "gateway"
        ? p.gatewayOnly
        : undefined;

  const toggle = (name: ManagedPackage, on: boolean) =>
    setChosen((current) => (on ? [...current, name] : current.filter((other) => other !== name)));

  const save = async () => {
    setSaving(true);
    try {
      // The latest document, so that the save is based on what another tab may have saved.
      const latest = await api.site.get();
      const { added, removed } = changeOf(managedOf(latest.config, props.node.id), chosen());
      const view = await api.site.save({
        base: latest.revision.id,
        message: p.message(props.node.name, added, removed),
        config: withManaged(latest.config, props.node.id, chosen()),
      });
      showToast({ title: p.saved(view.revision.id), description: p.savedHint, tone: "ok" });
    } catch (err) {
      showToast({ title: p.failed, description: errorText(err), tone: "crit" });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <CardHeader title={p.title} description={p.description} />
      <Show
        when={site.isSuccess}
        fallback={
          <p role="status" class="px-4 pb-4 text-fg-subtle text-sm">
            {site.isError ? errorText(site.error) : `${p.loading}…`}
          </p>
        }
      >
        <div class="flex flex-col gap-3 px-4 pb-4">
          <ul class="grid gap-x-6 gap-y-2.5 sm:grid-cols-2">
            <For each={PACKAGES}>
              {(name) => (
                <li>
                  <Checkbox
                    label={name}
                    hint={[p.names[name], unavailable(name)].filter(Boolean).join(". ")}
                    checked={chosen().includes(name)}
                    disabled={
                      !props.admin ||
                      saving() ||
                      // A package chosen where it can't be stays clearable.
                      (unavailable(name) !== undefined && !chosen().includes(name))
                    }
                    onChange={(on) => toggle(name, on)}
                  />
                </li>
              )}
            </For>
          </ul>
          <Show when={props.node.mode === "observe" && chosen().length > 0}>
            <p class="text-fg-subtle text-sm">{p.observeNote}</p>
          </Show>
          <Show when={props.admin}>
            <div class="flex justify-end gap-2">
              <Button
                variant="ghost"
                size="sm"
                disabled={!changed() || saving()}
                onClick={() => setChosen(saved())}
              >
                {p.reset}
              </Button>
              <Button variant="primary" size="sm" disabled={!changed() || saving()} onClick={save}>
                {p.save}
              </Button>
            </div>
          </Show>
        </div>
      </Show>
    </Card>
  );
}
