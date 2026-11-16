import { useSearchParams } from "@solidjs/router";
import { ExternalLink, RefreshCw } from "lucide";
import { createEffect, createMemo, createSignal, For, Show, useContext } from "solid-js";
import {
  createCatalog,
  type FirmwareRow,
  type FirmwareState,
  firmwareRows,
  preparable,
} from "../../lib/firmware";
import { formatDateTime, NONE } from "../../lib/format";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { type Column, Table } from "../../ui/Table";
import { ControllerCard } from "./ControllerCard";
import { SourceDialog, sourceLabel } from "./SourceDialog";
import { SourcesCard } from "./SourcesCard";
import { UpgradeDialog } from "./UpgradeDialog";
import { UploadsCard } from "./UploadsCard";

const t = strings.firmware;

const TONES: Record<FirmwareState, Tone> = {
  current: "ok",
  available: "info",
  listed: "info",
  unknown: "neutral",
};
const STATES: Record<FirmwareState, string> = {
  current: t.upToDate,
  available: t.available,
  listed: t.listed,
  unknown: t.unknown,
};

/**
 * Updates (D-121, D-122, D-123): the controller's own release (its card first), each device's OpenWrt release, its image source, the upgrades on offer
 * with their release notes (OpenWrt's, or its GitHub repo's newest releases), and for admins the
 * upgrade prepared with the device's packages (ASU's image or the official one), a GitHub release
 * or an uploaded image; below, the site's sources and the uploaded images. `?node=<id>` opens a
 * device's upgrade (its device page links here).
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
      ? rows().find((row) => String(row.firmware.node) === params.node && preparable(row))
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
  const listed = () => rows().filter((row) => row.state === "listed").length;
  const unknown = () => rows().filter((row) => row.state === "unknown").length;
  const siteRepo = () => catalog()?.sources.repo ?? null;
  /** The row whose source is being changed. */
  const [sourcing, setSourcing] = createSignal<FirmwareRow>();
  const name = (row: FirmwareRow) => row.node?.name ?? `#${row.firmware.node}`;

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
      class: "min-w-56",
      // Notes and release names are long: they wrap.
      cell: (row) => (
        <div class="whitespace-normal">
          <Show
            when={row.releases.length === 0}
            fallback={
              <Show when={row.releases[0]}>
                {(release) => (
                  <span class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm">
                    <span class="font-medium" title={release().name}>
                      {release().tag}
                    </span>
                    <Show when={release().prerelease}>
                      <Badge tone="warn">{t.prerelease}</Badge>
                    </Show>
                    <Show when={release().published_at}>
                      {(at) => <span class="text-fg-subtle text-xs">{formatDateTime(at())}</span>}
                    </Show>
                    <a
                      href={release().notes}
                      target="_blank"
                      rel="noopener noreferrer"
                      aria-label={`${t.notes} ${release().tag}`}
                      class="inline-flex items-center gap-1 text-accent text-xs hover:underline"
                    >
                      {t.notes}
                      <Icon icon={ExternalLink} class="size-3" />
                    </a>
                    <Show when={row.releases.length > 1}>
                      <span class="text-fg-subtle text-xs">{t.older(row.releases.length - 1)}</span>
                    </Show>
                  </span>
                )}
              </Show>
            }
          >
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
          </Show>
        </div>
      ),
    },
    {
      id: "source",
      header: t.columns.source,
      class: "min-w-52 max-w-64",
      cell: (row) => (
        <span class="flex flex-col items-start gap-0.5 whitespace-normal">
          <span class="break-words text-fg-muted text-xs">
            {sourceLabel(row.firmware.source, siteRepo())}
          </span>
          <Show when={admin()}>
            <button
              type="button"
              class="rounded-control text-accent text-xs hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
              aria-label={`${t.source.change}: ${name(row)}`}
              onClick={() => setSourcing(row)}
            >
              {t.source.change}
            </button>
          </Show>
        </span>
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
        <Show when={admin() && preparable(row)}>
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

  const fits = (sha256: string) =>
    rows().filter((row) => row.uploads.some((upload) => upload.sha256 === sha256)).length;

  return (
    <div class="flex flex-col gap-4">
      <ControllerCard admin={admin()} />
      <Card class="mx-auto w-full max-w-7xl overflow-hidden" aria-label={t.title}>
        <CardHeader
          title={t.title}
          description={
            catalog() && (!catalog()?.error || listed() > 0)
              ? t.summary(rows().length, upgradable(), listed(), unknown())
              : t.description
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
              uploads={row().uploads}
              releases={row().releases}
              repo={row().firmware.repo}
              source={row().firmware.source}
              managed={row().node?.mode === "managed"}
            />
          )}
        </Show>
      </Card>
      <Show when={sourcing()}>
        {(row) => (
          <SourceDialog
            open
            onOpenChange={(open) => {
              if (!open) setSourcing(undefined);
            }}
            node={row().firmware.node}
            name={name(row())}
            source={row().firmware.source}
            siteRepo={siteRepo()}
            onSaved={reload}
          />
        )}
      </Show>
      <Show when={catalog()}>
        {(found) => <SourcesCard sources={found().sources} admin={admin()} onChange={reload} />}
      </Show>
      <Show when={catalog()}>
        {(found) => (
          <UploadsCard uploads={found().uploads} fits={fits} admin={admin()} onChange={reload} />
        )}
      </Show>
    </div>
  );
}
