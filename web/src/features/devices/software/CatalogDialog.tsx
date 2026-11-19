import { LoaderCircle, RefreshCw } from "lucide";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { AvailablePackage } from "../../../api/gen/AvailablePackage";
import type { InstalledPackage } from "../../../api/gen/InstalledPackage";
import type { NodeId } from "../../../api/gen/NodeId";
import type { PackageCatalog } from "../../../api/gen/PackageCatalog";
import type { PackageChanges } from "../../../api/gen/PackageChanges";
import { api, errorText } from "../../../lib/api";
import { formatBytes, formatDateTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { type Column, Table } from "../../../ui/Table";
import { TextField } from "../../../ui/TextField";
import { type ChangeKind, changeCount, offered, searched, upgradeBlock } from "./software";

const c = strings.device.software.catalog;

export interface CatalogRead {
  catalog?: PackageCatalog;
  reading: boolean;
  error?: string;
}

/**
 * The node's catalog, read on request (it can take a minute) and kept until forgotten, as after
 * a change of its packages. A read still under way when forgotten or asked again is ignored.
 */
export function createCatalogReader(node: () => NodeId) {
  const [state, setState] = createSignal<CatalogRead>({ reading: false });
  let latest = 0;
  const read = async () => {
    const ask = ++latest;
    setState((now) => ({ catalog: now.catalog, reading: true }));
    try {
      const catalog = await api.nodes.packageCatalog(node());
      if (ask === latest) setState({ catalog, reading: false });
    } catch (err) {
      if (ask === latest) {
        setState((now) => ({ catalog: now.catalog, reading: false, error: errorText(err) }));
      }
    }
  };
  const forget = () => {
    latest += 1;
    setState({ reading: false });
  };
  return { state, read, forget };
}

export type CatalogReader = ReturnType<typeof createCatalogReader>;

/**
 * Picking installs and upgrades from the node's feeds (network.md, Package manager; D-139): the
 * upgrades on offer (those that come with the firmware can't be chosen), then what the feeds hold
 * that isn't installed, searched by name or description. Choices go into the card's changes.
 */
export function CatalogDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  name: string;
  reader: CatalogReader;
  installed: readonly InstalledPackage[];
  changes: PackageChanges;
  onToggle: (kind: ChangeKind, name: string, on: boolean) => void;
}) {
  const [query, setQuery] = createSignal("");
  const state = () => props.reader.state();

  createEffect(
    () => props.open,
    (open) => {
      const now = props.reader.state();
      if (open && !now.catalog && !now.reading) void props.reader.read();
    },
  );

  const rows = createMemo(() => {
    const catalog = state().catalog;
    return catalog ? searched(offered(catalog, props.installed), query()) : [];
  });

  const columns: Column<AvailablePackage>[] = [
    {
      id: "pick",
      header: c.columns.pick,
      class: "w-16",
      cell: (pkg) => (
        <Checkbox
          hideLabel
          label={c.install(pkg.name)}
          checked={props.changes.install.includes(pkg.name)}
          onChange={(on) => props.onToggle("install", pkg.name, on)}
        />
      ),
    },
    {
      id: "name",
      header: c.columns.name,
      cell: (pkg) => <span class="font-mono text-sm">{pkg.name}</span>,
    },
    {
      id: "version",
      header: c.columns.version,
      cell: (pkg) => <span class="font-mono text-fg-muted text-xs">{pkg.version}</span>,
    },
    {
      id: "size",
      header: c.columns.size,
      align: "end",
      cell: (pkg) => <span class="text-fg-muted">{formatBytes(pkg.installed_bytes)}</span>,
    },
    {
      id: "about",
      header: c.columns.about,
      class: "max-w-sm",
      cell: (pkg) => (
        <span class="block truncate text-fg-muted" title={pkg.description}>
          {pkg.description}
        </span>
      ),
    },
  ];

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={c.title(props.name)}
      description={c.description}
      size="wide"
    >
      <div class="flex flex-col gap-4 px-5 pt-2 pb-5">
        <Show when={state().reading}>
          <div role="status" class="flex items-start gap-3 rounded-control border border-edge p-3">
            <Icon icon={LoaderCircle} class="mt-0.5 size-4 shrink-0 animate-spin text-accent" />
            <div class="text-sm">
              <p>{c.reading}</p>
              <p class="mt-0.5 text-fg-subtle text-xs">{c.readingHint}</p>
            </div>
          </div>
        </Show>
        <RequestError text={state().error} />
        <Show when={!state().catalog && !state().reading && state().error}>
          <div>
            <Button variant="secondary" size="sm" onClick={() => void props.reader.read()}>
              {c.retry}
            </Button>
          </div>
        </Show>
        <Show when={state().catalog}>
          {(catalog) => (
            <>
              <div class="flex items-center justify-between gap-2 text-fg-subtle text-xs">
                <span>{c.readAt(formatDateTime(catalog().read_at))}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  icon={RefreshCw}
                  disabled={state().reading}
                  onClick={() => void props.reader.read()}
                >
                  {c.again}
                </Button>
              </div>
              <section class="flex flex-col gap-2" aria-label={c.upgrades}>
                <h3 class="font-medium text-sm">{c.upgrades}</h3>
                <Show
                  when={catalog().upgrades.length > 0}
                  fallback={<p class="text-fg-subtle text-sm">{c.noUpgrades}</p>}
                >
                  <ul class="grid gap-x-6 gap-y-2.5 sm:grid-cols-2">
                    <For each={catalog().upgrades}>
                      {(up) => {
                        const blocked = () => upgradeBlock(up.name, props.installed);
                        return (
                          <li>
                            <Checkbox
                              label={up.name}
                              hint={[c.upgrade(up.from, up.to), blocked()]
                                .filter(Boolean)
                                .join(". ")}
                              checked={props.changes.upgrade.includes(up.name)}
                              disabled={blocked() !== undefined}
                              onChange={(on) => props.onToggle("upgrade", up.name, on)}
                            />
                          </li>
                        );
                      }}
                    </For>
                  </ul>
                </Show>
              </section>
              <section class="flex flex-col gap-2" aria-label={c.available}>
                <h3 class="font-medium text-sm">{c.available}</h3>
                <TextField
                  label={c.search}
                  hint={c.searchHint}
                  value={query()}
                  onInput={setQuery}
                />
                <Table
                  label={c.available}
                  columns={columns}
                  rows={rows()}
                  rowKey={(pkg) => pkg.name}
                  empty={c.none}
                  class="max-h-[45vh] rounded-control border border-edge"
                />
              </section>
            </>
          )}
        </Show>
        <div class="flex items-center justify-between gap-2">
          <p class="text-fg-muted text-sm" role="status">
            {c.chosen(changeCount(props.changes))}
          </p>
          <Button variant="primary" onClick={() => props.onOpenChange(false)}>
            {c.done}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
