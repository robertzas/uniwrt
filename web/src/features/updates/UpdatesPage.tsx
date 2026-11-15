import { useSearchParams } from "@solidjs/router";
import { ExternalLink, RefreshCw } from "lucide";
import { createEffect, createMemo, For, Show, useContext } from "solid-js";
import {
  createCatalog,
  type FirmwareRow,
  type FirmwareState,
  firmwareRows,
} from "../../lib/firmware";
import { NONE } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { type Column, Table } from "../../ui/Table";
import { UpgradeDialog } from "./UpgradeDialog";

const t = strings.firmware;

const TONES: Record<FirmwareState, Tone> = { current: "ok", available: "info", unknown: "neutral" };
const STATES: Record<FirmwareState, string> = {
  current: t.upToDate,
  available: t.available,
  unknown: t.unknown,
};

/**
 * Updates (D-121): each device's OpenWrt release, the upgrades on offer with their release
 * notes, and for admins the upgrade image prepared with the device's packages. `?node=<id>`
 * opens a device's upgrade (its device page links here).
 */
export function UpdatesPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const [params, setParams] = useSearchParams<{ node?: string }>();
  const admin = () => session.user.role === "admin";
  const { catalog, error, loading, reload } = createCatalog();
  const rows = createMemo(() => {
    const found = catalog();
    return found ? firmwareRows(found, live.fleet.nodes) : [];
  });
  /** The row whose upgrade is open: the one `?node=` names, when an admin can prepare it. */
  const upgrading = () =>
    admin()
      ? rows().find((row) => String(row.firmware.node) === params.node && row.buildable.length > 0)
      : undefined;
  const setUpgrading = (row: FirmwareRow | undefined) =>
    setParams({ node: row ? String(row.firmware.node) : undefined }, { replace: true });
  createEffect(
    () => strings.nav.updates.label,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );
  const upgradable = () => rows().filter((row) => row.state === "available").length;

  const columns: Column<FirmwareRow>[] = [
    {
      id: "device",
      header: t.columns.device,
      cell: (row) => (
        <span class="flex min-w-0 flex-col">
          <span class="truncate font-medium">{row.node?.name ?? `#${row.firmware.node}`}</span>
          <span class="truncate text-fg-subtle text-xs">
            {row.node?.model ?? row.firmware.board}
          </span>
        </span>
      ),
      class: "min-w-40",
    },
    {
      id: "running",
      header: t.columns.running,
      cell: (row) => (
        <span class="flex flex-col">
          <span class="whitespace-nowrap">{row.firmware.version || NONE}</span>
          <span class="font-mono text-fg-subtle text-xs">{row.firmware.revision}</span>
        </span>
      ),
    },
    {
      id: "upgrade",
      header: t.columns.upgrade,
      cell: (row) => (
        <Show
          when={row.firmware.offers.length > 0}
          fallback={<span class="text-fg-subtle text-xs">{row.firmware.note ?? NONE}</span>}
        >
          <ul class="flex flex-col gap-1">
            <For each={row.firmware.offers}>
              {(offer) => (
                <li class="flex flex-wrap items-center gap-x-2 text-sm">
                  <span class="font-medium tabular-nums">{offer.release}</span>
                  <span class="text-fg-muted text-xs">{t.kind[offer.kind] ?? offer.kind}</span>
                  <Show
                    when={offer.profile !== null}
                    fallback={<span class="text-fg-subtle text-xs">{t.noImage}</span>}
                  >
                    <a
                      href={offer.notes}
                      target="_blank"
                      rel="noopener noreferrer"
                      aria-label={`${t.notes} ${offer.release}`}
                      class="inline-flex items-center gap-1 text-accent text-xs hover:underline"
                    >
                      {t.notes}
                      <Icon icon={ExternalLink} class="size-3" />
                    </a>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      ),
    },
    {
      id: "state",
      header: t.columns.state,
      cell: (row) => (
        <span title={row.firmware.note ?? undefined}>
          <Badge tone={TONES[row.state]}>{STATES[row.state]}</Badge>
        </span>
      ),
    },
    {
      id: "action",
      header: "",
      cell: (row) => (
        <Show when={admin() && row.buildable.length > 0}>
          <Button
            size="sm"
            aria-label={`${t.prepare}: ${row.node?.name ?? row.firmware.node}`}
            onClick={() => setUpgrading(row)}
          >
            {t.prepare}
          </Button>
        </Show>
      ),
      align: "end",
    },
  ];

  return (
    <Card class="mx-auto max-w-7xl overflow-hidden" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={
          catalog() && !catalog()?.error ? t.summary(rows().length, upgradable()) : t.description
        }
        actions={
          <Button size="sm" icon={RefreshCw} disabled={loading()} onClick={reload}>
            {t.refresh}
          </Button>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-2">
        <Show when={catalog() && !catalog()?.error}>
          <p class="text-fg-muted text-sm">{t.description}</p>
        </Show>
        <RequestError text={error() ?? catalog()?.error ?? undefined} />
      </div>
      <Show
        when={catalog()}
        fallback={
          <Show when={!error()}>
            <p role="status" class="py-12 text-center text-fg-subtle text-sm">
              {t.loading}
            </p>
          </Show>
        }
      >
        <Table
          label={t.title}
          columns={columns}
          rows={rows()}
          rowKey={(row) => String(row.firmware.node)}
        />
      </Show>
      <Show when={upgrading()}>
        {(row) => (
          <UpgradeDialog
            open
            onOpenChange={(open) => {
              if (!open) setUpgrading(undefined);
            }}
            node={row().firmware.node}
            name={row().node?.name ?? `#${row().firmware.node}`}
            offers={row().buildable}
          />
        )}
      </Show>
    </Card>
  );
}
