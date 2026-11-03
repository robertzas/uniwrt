import { For, Show } from "solid-js";
import type { BandReport } from "../../api/gen/BandReport";
import type { MatrixEntry } from "../../api/gen/MatrixEntry";
import { formatDbm } from "../../lib/format";
import { linkTone } from "../../lib/rf";
import { strings } from "../../lib/strings";
import type { Tone } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";

const t = strings.rf.matrix;

/** A cell's tint by how well the link carries: a tint, so the number stays readable. */
const CELL: Record<Tone, string> = {
  ok: "bg-ok/15",
  warn: "bg-warn/20",
  crit: "bg-crit/15",
  info: "bg-info/15",
  neutral: "",
};

/** The nodes of the band's matrix and radios, by name. */
export function matrixNodes(band: BandReport, nodeName: (id: number) => string): number[] {
  const ids = new Set<number>();
  for (const e of band.matrix) ids.add(e.from).add(e.to);
  for (const r of band.radios) ids.add(r.node);
  return [...ids].sort((a, b) => nodeName(a).localeCompare(nodeName(b)) || a - b);
}

/**
 * The inter-node RSSI matrix: each row is the device that hears, each column the device heard;
 * live mesh links are marked. The link budget behind roaming and the channel planner.
 */
export function Matrix(props: { band: BandReport; nodeName: (id: number) => string }) {
  const nodes = () => matrixNodes(props.band, props.nodeName);
  const entry = (from: number, to: number): MatrixEntry | undefined =>
    props.band.matrix.find((e) => e.from === from && e.to === to);
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Show
        when={props.band.matrix.length > 0}
        fallback={<p class="px-4 pb-4 text-fg-subtle text-sm">{t.empty}</p>}
      >
        <div class="overflow-x-auto px-4 pb-4">
          <table class="w-full border-separate border-spacing-1 text-xs">
            <thead>
              <tr>
                <th scope="col" class="text-left font-normal text-fg-subtle">
                  {t.heardBy} ↓ · {t.heard} →
                </th>
                <For each={nodes()}>
                  {(id) => (
                    <th scope="col" class="max-w-28 truncate px-1 text-center font-medium">
                      {props.nodeName(id)}
                    </th>
                  )}
                </For>
              </tr>
            </thead>
            <tbody>
              <For each={nodes()}>
                {(from) => (
                  <tr>
                    <th scope="row" class="max-w-36 truncate pr-2 text-left font-medium">
                      {props.nodeName(from)}
                    </th>
                    <For each={nodes()}>
                      {(to) => {
                        const e = () => entry(from, to);
                        return (
                          <td
                            class={[
                              "h-9 min-w-16 rounded-control text-center font-mono tabular-nums",
                              from === to
                                ? "bg-raised/60"
                                : e()
                                  ? CELL[linkTone(e()?.signal_dbm ?? 0)]
                                  : "",
                            ]}
                            title={
                              e()
                                ? t.cell(
                                    props.nodeName(from),
                                    props.nodeName(to),
                                    formatDbm(e()?.signal_dbm),
                                    e()?.source === "mesh" ? t.mesh : t.scan,
                                  )
                                : undefined
                            }
                          >
                            <Show when={from !== to}>
                              <Show when={e()} fallback={<span class="text-fg-subtle">·</span>}>
                                {(found) => (
                                  <span class="inline-flex items-center gap-1">
                                    {found().signal_dbm}
                                    <Show when={found().source === "mesh"}>
                                      <span
                                        class="size-1.5 rounded-full bg-accent"
                                        role="img"
                                        aria-label={t.mesh}
                                      />
                                    </Show>
                                  </span>
                                )}
                              </Show>
                            </Show>
                          </td>
                        );
                      }}
                    </For>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
          <p class="mt-2 flex items-center gap-1.5 text-fg-subtle text-xs">
            <span class="size-1.5 rounded-full bg-accent" aria-hidden="true" />
            {t.mesh}
            <span class="ml-3">dBm</span>
          </p>
        </div>
      </Show>
    </Card>
  );
}
