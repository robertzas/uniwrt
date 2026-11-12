import { useQuery } from "@tanstack/solid-query";
import { Cable, TriangleAlert } from "lucide";
import { createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import { api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { EmptyState } from "../../../ui/EmptyState";
import { INPUT } from "../../../ui/FormField";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { Segmented } from "../../../ui/Segmented";
import {
  carriesOf,
  type PortDraft,
  portDraft,
  portsOf,
  profileOf,
  untaggedOf,
  withPorts,
} from "./ports";

const t = strings.ports;

/** A swconfig switch's port (`switch0:3`), which can't carry the backhaul yet (mesh.md). */
const onSwitch = (port: string) => port.includes(":");

/**
 * A device's Ports tab (network.md, Networks; D-077): each LAN port with its link and the
 * networks it carries, untagged and tagged; admins change them as the site's next revision.
 */
export function PortsTab(props: { node: Node; facts: NodeFacts | null }) {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const admin = () => session.user.role === "admin";
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  const networks = () => site.data?.config.networks ?? [];
  const entry = () => site.data?.config.nodes.find((n) => n.node === props.node.id);
  const profiles = () => entry()?.ports ?? [];
  const untagged = () => untaggedOf(networks());
  const vlans = () => networks().filter((n) => n.vlan !== null);
  /** Whether the site's mesh runs over cables at all (D-094). */
  const wired = () => {
    const mesh = site.data?.config.mesh;
    return !!mesh && mesh.enabled && mesh.wired;
  };
  const rows = createMemo(() => portsOf(props.facts, profiles()));
  const [drafts, setDrafts] = createSignal<Record<string, PortDraft>>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const draftOf = (port: string) =>
    drafts()?.[port] ??
    portDraft(
      profiles().find((p) => p.port === port),
      untagged(),
    );

  const edit = () => {
    setDrafts(Object.fromEntries(rows().map((row) => [row.port, draftOf(row.port)])));
    setFailure(undefined);
    setSaved(undefined);
  };
  const update = (port: string, next: Partial<PortDraft>) =>
    setDrafts((now) => (now ? { ...now, [port]: { ...draftOf(port), ...next } } : now));

  const save = async () => {
    const now = drafts();
    if (!now) return;
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => {
        const untaggedId = untaggedOf(config.networks);
        const ports = Object.entries(now)
          .map(([port, draft]) => profileOf(port, draft, untaggedId))
          .filter((profile) => profile !== null);
        return withPorts(config, props.node.id, ports);
      }, t.message(props.node.name));
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
          <Show when={admin() && site.isSuccess && rows().length > 0 && !drafts()}>
            <Button size="sm" onClick={edit}>
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show when={site.isSuccess && !(entry()?.managed ?? []).includes("network")}>
          <p class="flex items-start gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-sm">
            <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
            {t.unmanaged}
          </p>
        </Show>
        <Show
          when={site.isSuccess}
          fallback={
            <p role="status" class="text-fg-subtle text-sm">
              {site.isError ? errorText(site.error) : `${t.loading}…`}
            </p>
          }
        >
          <Show
            when={rows().length > 0}
            fallback={<EmptyState icon={Cable} title={t.title} description={t.none} />}
          >
            <Show when={vlans().length === 0}>
              <p class="text-fg-muted text-xs">{t.noVlans}</p>
            </Show>
            <p class="text-fg-muted text-xs">{wired() ? t.backhaulNote : t.wiredOff}</p>
            <div class="overflow-x-auto">
              <table class="w-full text-sm">
                <thead class="text-left text-fg-muted text-xs">
                  <tr>
                    <th class="py-1.5 pr-3 font-medium">{t.port}</th>
                    <th class="py-1.5 pr-3 font-medium">{t.link}</th>
                    <th class="py-1.5 pr-3 font-medium">{t.native}</th>
                    <th class="py-1.5 pr-3 font-medium">{t.tagged}</th>
                    <th class="py-1.5 font-medium">{t.backhaul}</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-edge">
                  <For each={rows()} keyed={(row) => row.port}>
                    {(row) => (
                      <tr class="align-top" aria-label={row().port}>
                        <td class="py-2 pr-3 font-mono text-xs leading-7">
                          {row().port}
                          <Show when={!row().present}>
                            <p class="font-sans text-warn text-xs leading-4">{t.missing}</p>
                          </Show>
                        </td>
                        <td class="py-2 pr-3 text-xs leading-7">
                          <Show when={row().link} fallback={<span class="text-fg-subtle">—</span>}>
                            {(link) => (
                              <Badge tone={link().carrier ? "ok" : "neutral"}>
                                {link().carrier ? t.speed(link().speed) : t.down}
                              </Badge>
                            )}
                          </Show>
                        </td>
                        <Show
                          when={drafts()}
                          fallback={
                            <>
                              <td colspan={2} class="py-2 pr-3 text-xs leading-7">
                                {carriesOf(draftOf(row().port), untagged(), networks())}
                              </td>
                              <td class="py-2 text-xs leading-7">
                                <Show
                                  when={!onSwitch(row().port)}
                                  fallback={<span class="text-fg-subtle">—</span>}
                                >
                                  <span class={draftOf(row().port).backhaul ? "" : "text-fg-muted"}>
                                    {draftOf(row().port).backhaul ? t.backhaulOn : t.backhaulOff}
                                  </span>
                                </Show>
                              </td>
                            </>
                          }
                        >
                          <td class="py-2 pr-3">
                            <select
                              aria-label={`${t.native} ${row().port}`}
                              class={[INPUT, "max-w-52 border-edge"]}
                              value={draftOf(row().port).native}
                              onChange={(event) =>
                                update(row().port, { native: event.currentTarget.value })
                              }
                            >
                              <option value="">{t.nativeNone}</option>
                              <For each={networks()}>
                                {(network) => <option value={network.id}>{network.name}</option>}
                              </For>
                            </select>
                          </td>
                          <td class="py-2 pr-3">
                            <Show when={vlans().length > 0} fallback={<span>—</span>}>
                              <Segmented
                                label={`${t.tagged} ${row().port}`}
                                value={draftOf(row().port).tagged}
                                options={(["all", "none", "only"] as const).map((value) => ({
                                  value,
                                  label: t.taggedModes[value] ?? value,
                                }))}
                                onChange={(tagged) => update(row().port, { tagged })}
                                class="w-fit"
                              />
                              <Show when={draftOf(row().port).tagged === "only"}>
                                <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1.5">
                                  <For
                                    each={vlans().filter(
                                      (n) => n.id !== draftOf(row().port).native,
                                    )}
                                  >
                                    {(network) => (
                                      <Checkbox
                                        label={network.name}
                                        checked={draftOf(row().port).only.includes(network.id)}
                                        onChange={(on) => {
                                          const only = draftOf(row().port).only.filter(
                                            (id) => id !== network.id,
                                          );
                                          update(row().port, {
                                            only: on ? [...only, network.id] : only,
                                          });
                                        }}
                                      />
                                    )}
                                  </For>
                                </div>
                              </Show>
                            </Show>
                          </td>
                          <td class="py-2 leading-7">
                            <Show
                              when={!onSwitch(row().port)}
                              fallback={
                                <span class="text-fg-subtle text-xs" title={t.switchPort}>
                                  —
                                </span>
                              }
                            >
                              <Checkbox
                                label={t.backhaulToggle(row().port)}
                                hideLabel
                                checked={draftOf(row().port).backhaul}
                                onChange={(backhaul) => update(row().port, { backhaul })}
                                class="pt-1.5"
                              />
                            </Show>
                          </td>
                        </Show>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
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
          </Show>
        </Show>
        <Show when={!admin()}>
          <p class="text-fg-subtle text-xs">{t.adminOnly}</p>
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
    </Card>
  );
}
