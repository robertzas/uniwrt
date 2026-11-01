import { Show } from "solid-js";
import type { ClientSample } from "../../api/gen/ClientSample";
import { formatBitrate, formatDbm, formatRate } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Card } from "../../ui/Card";
import { LineChart } from "./LineChart";

const s = strings.client.signal;

/**
 * The Signal tab: the last hour of the client's signal, PHY rates and throughput, a sample every
 * few seconds while it is online over Wi-Fi (kept in the controller's memory until M2.4's metrics).
 */
export function SignalTab(props: { samples: readonly ClientSample[] }) {
  const times = () => props.samples.map((sample) => sample.at);
  const pick = (key: keyof Omit<ClientSample, "at">) => props.samples.map((sample) => sample[key]);
  return (
    <Show
      when={props.samples.length > 1}
      fallback={<p class="py-12 text-center text-fg-subtle text-sm">{s.empty}</p>}
    >
      <div class="grid gap-4 xl:grid-cols-2">
        <Card class="p-4 xl:col-span-2">
          <LineChart
            title={s.title}
            times={times()}
            series={[{ label: s.series.signal, class: "text-info", values: pick("signal_dbm") }]}
            format={(value) => formatDbm(value)}
            minSpan={20}
          />
        </Card>
        <Card class="p-4">
          <LineChart
            title={s.rates}
            times={times()}
            series={[
              { label: s.series.tx, class: "text-accent", values: pick("tx_kbps") },
              { label: s.series.rx, class: "text-warn", values: pick("rx_kbps") },
            ]}
            format={(value) => formatRate(value)}
            min={0}
          />
        </Card>
        <Card class="p-4">
          <LineChart
            title={s.throughput}
            times={times()}
            series={[
              { label: s.series.down, class: "text-ok", values: pick("down_bps") },
              { label: s.series.up, class: "text-info", values: pick("up_bps") },
            ]}
            format={(value) => formatBitrate(value)}
            min={0}
          />
        </Card>
      </div>
    </Show>
  );
}
