import { TriangleAlert } from "lucide";
import { createMemo, createSignal, For, Show } from "solid-js";
import type { PortMapping } from "../../api/gen/PortMapping";
import type { SiteView } from "../../api/gen/SiteView";
import type { Upnp } from "../../api/gen/Upnp";
import { errorText } from "../../lib/api";
import { formatDuration } from "../../lib/format";
import { saveSite, sortNetworks } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { type Column, Table } from "../../ui/Table";

const t = strings.security.upnp;

/**
 * UPnP IGD and NAT-PMP/PCP (network.md, Firewall policy): on or off, the networks whose clients
 * may open ports, saved as one revision, and the ports open now (the gateway's `port_mappings`).
 */
export function UpnpCard(props: {
  view: SiteView;
  admin: boolean;
  mappings: readonly PortMapping[];
  /** The gateway's name, when it doesn't manage `upnpd`. */
  unmanaged: string | undefined;
  hostName: (ip: string) => string | undefined;
}) {
  const [draft, setDraft] = createSignal<Upnp>();
  const [error, setError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const upnp = () => draft() ?? props.view.config.firewall.upnp;
  const networks = createMemo(() => sortNetworks(props.view.config.networks));
  const editing = () => draft() !== undefined;

  const change = (next: Partial<Upnp>) => {
    setDraft((now) => now && { ...now, ...next });
    setError(undefined);
  };
  const toggleNetwork = (id: string, on: boolean) => {
    const now = upnp().networks.filter((n) => n !== id);
    change({ networks: on ? [...now, id] : now });
  };

  const save = async () => {
    const next = draft();
    if (!next) return;
    if (next.enabled && next.networks.length === 0) {
      setError(t.noNetworks);
      return;
    }
    const before = props.view.config.firewall.upnp;
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, firewall: { ...config.firewall, upnp: next } }),
        next.enabled === before.enabled ? t.networksMessage : t.message(next.enabled),
      );
      setSaved(view.revision.id);
      setDraft(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const columns: Column<PortMapping>[] = [
    { id: "proto", header: t.columns.proto, cell: (m) => m.proto.toUpperCase(), class: "w-20" },
    { id: "port", header: t.columns.port, cell: (m) => m.external_port, align: "end" },
    {
      id: "client",
      header: t.columns.client,
      cell: (m) => (
        <span>
          <span class="font-mono">{m.ip}</span>
          <Show when={props.hostName(m.ip)}>
            {(name) => <span class="text-fg-muted"> · {name()}</span>}
          </Show>
        </span>
      ),
    },
    { id: "client_port", header: t.columns.clientPort, cell: (m) => m.internal_port, align: "end" },
    { id: "description", header: t.columns.description, cell: (m) => m.description || "—" },
    {
      id: "expires",
      header: t.columns.expires,
      cell: (m) => (m.expires_in_s === null ? t.never : formatDuration(m.expires_in_s)),
      align: "end",
    },
  ];

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && !editing()}>
            <Button
              size="sm"
              onClick={() => {
                setDraft({ ...props.view.config.firewall.upnp });
                setError(undefined);
                setSaved(undefined);
              }}
            >
              {strings.security.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show when={props.unmanaged && upnp().enabled}>
          <div class="flex items-center gap-3 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
            <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
            <p class="min-w-0 flex-1">{t.unmanaged(props.unmanaged ?? "")}</p>
          </div>
        </Show>
        <Checkbox
          label={t.enabled}
          checked={upnp().enabled}
          disabled={!editing() || busy()}
          onChange={(enabled) => change({ enabled })}
        />
        <fieldset class="flex flex-col gap-2 pl-7" disabled={!editing() || busy()}>
          <legend class="mb-1 font-medium text-fg-muted text-xs">{t.networks}</legend>
          <For each={networks()}>
            {(network) => (
              <Checkbox
                label={network.name}
                hint={network.ipv4 ? undefined : t.noIpv4}
                checked={upnp().networks.includes(network.id)}
                disabled={
                  !editing() || busy() || (!network.ipv4 && !upnp().networks.includes(network.id))
                }
                onChange={(on) => toggleNetwork(network.id, on)}
              />
            )}
          </For>
          <Show when={error()}>
            <p class="text-crit text-xs">{error()}</p>
          </Show>
        </fieldset>
        <Show when={editing()}>
          <div class="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
              {strings.security.cancel}
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={busy()}>
              {strings.security.save}
            </Button>
          </div>
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
        <div class="mt-2 flex items-baseline gap-2">
          <h3 class="font-medium text-sm">{t.mappings}</h3>
          <span class="text-fg-subtle text-xs">{t.mappingsHint}</span>
        </div>
        <Table
          label={t.mappingsLabel}
          columns={columns}
          rows={props.mappings}
          rowKey={(m) => `${m.proto}/${m.external_port}`}
          empty={
            <p class="px-3 py-4 text-fg-muted text-sm">
              {props.view.config.firewall.upnp.enabled ? t.mappingsNone : t.mappingsOff}
            </p>
          }
        />
      </div>
    </Card>
  );
}
