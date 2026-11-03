import { createSignal, Show } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import type { Neighbor } from "../../api/gen/Neighbor";
import { formatDbm, NONE } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";
import { Segmented } from "../../ui/Segmented";
import { type Column, Table } from "../../ui/Table";
import { networkName } from "./Spectrum";

const t = strings.rf.networks;

type Filter = "all" | "foreign" | "yours";

const KIND_TONE: Record<Neighbor["kind"], Tone> = {
  own: "info",
  lookalike: "warn",
  foreign: "neutral",
};

/** Every network heard on the band: who it is, where, how strong, how protected, who heard it. */
export function Networks(props: {
  band: BandReport;
  nodeName: (id: number) => string;
  selected: string | undefined;
  onSelect: (bssid: string | undefined) => void;
}) {
  const [filter, setFilter] = createSignal<Filter>("all");
  const rows = () =>
    props.band.networks.filter((n) =>
      filter() === "all" ? true : filter() === "yours" ? n.kind === "own" : n.kind !== "own",
    );
  const columns: Column<Neighbor>[] = [
    {
      id: "name",
      header: t.name,
      cell: (n) => (
        <span class="flex min-w-0 flex-col">
          <span class="flex items-center gap-2">
            <span
              class={[
                "truncate",
                n.bss.ssid === null && n.bss.mesh_id === null && n.kind !== "own"
                  ? "text-fg-muted italic"
                  : "font-medium",
                props.selected === n.bss.bssid ? "text-accent" : "",
              ]}
            >
              {networkName(n, props.nodeName)}
            </span>
            <span title={n.kind === "lookalike" ? t.lookalikeHint : undefined}>
              <Badge tone={KIND_TONE[n.kind]}>{t.kind[n.kind]}</Badge>
            </span>
          </span>
          <Show when={n.kind === "own" ? n.bss.mesh_id : null}>
            {(id) => <span class="text-fg-subtle text-xs">{t.mesh(id())}</span>}
          </Show>
        </span>
      ),
    },
    {
      id: "bssid",
      header: t.bssid,
      class: "w-44",
      cell: (n) => (
        <span class="flex flex-col">
          <span class="font-mono text-xs">{n.bss.bssid}</span>
          <span class="truncate text-fg-subtle text-xs">{n.vendor ?? NONE}</span>
        </span>
      ),
    },
    {
      id: "channel",
      header: t.channel,
      align: "end",
      class: "w-28",
      cell: (n) => `${n.bss.channel} · ${n.bss.width} MHz`,
    },
    {
      id: "signal",
      header: t.signal,
      align: "end",
      class: "w-24",
      cell: (n) => formatDbm(n.bss.signal_dbm),
    },
    {
      id: "security",
      header: t.security,
      class: "w-32",
      cell: (n) => t.securityKind[n.bss.security],
    },
    {
      id: "standard",
      header: t.standard,
      class: "w-24",
      cell: (n) => strings.device.radios.standard[n.bss.standard],
    },
    {
      id: "heard",
      header: t.heardBy,
      class: "w-40",
      cell: (n) => (
        <span class="truncate text-xs">
          {[...new Set(n.heard_by.map((h) => props.nodeName(h.node)))].join(", ")}
        </span>
      ),
    },
    {
      id: "impact",
      header: t.impact,
      align: "end",
      class: "w-20",
      cell: (n) => (n.kind === "own" || n.impact === 0 ? NONE : n.impact.toFixed(2)),
    },
  ];
  return (
    <Card class="min-w-0" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Segmented
            label={t.filterLabel}
            value={filter()}
            onChange={setFilter}
            options={[
              { value: "all", label: t.filter.all },
              { value: "foreign", label: t.filter.foreign },
              { value: "yours", label: t.filter.yours },
            ]}
          />
        }
      />
      <div class="px-2 pb-2">
        <Table
          label={t.label}
          columns={columns}
          rows={rows()}
          rowKey={(n) => n.bss.bssid}
          onRowClick={(n) => props.onSelect(n.bss.bssid)}
          empty={<p class="py-8 text-center text-fg-subtle text-sm">{t.empty}</p>}
        />
      </div>
    </Card>
  );
}
