import { Fingerprint, Terminal } from "lucide";
import type { ProbeResult } from "../../../api/gen/ProbeResult";
import { strings } from "../../../lib/strings";
import { Icon } from "../../../ui/Icon";
import { formatAddress } from "./address";

const s = strings.adopt.confirm;

/** A probed device's host key fingerprint to compare, and how to read the device's own. */
export function HostKey(props: { probe: ProbeResult }) {
  return (
    <>
      <div class="rounded-control border border-accent/40 bg-accent/5 px-3.5 py-3">
        <p class="flex items-center gap-1.5 text-fg-muted text-xs">
          <Icon icon={Fingerprint} class="size-3.5 text-accent" />
          {s.fingerprint(props.probe.algorithm)} ·{" "}
          <span class="font-mono">{formatAddress(props.probe.host, props.probe.port)}</span>
        </p>
        <p class="mt-1.5 break-all font-medium font-mono text-fg text-sm" data-testid="fingerprint">
          {props.probe.fingerprint}
        </p>
      </div>
      <div class="text-fg-muted text-xs">
        <p>{s.howTo}</p>
        <p class="mt-1 flex items-center gap-1.5 overflow-x-auto rounded-control bg-raised px-2.5 py-1.5 font-mono text-fg">
          <Icon icon={Terminal} class="size-3.5 shrink-0 text-fg-subtle" />
          <span class="whitespace-nowrap">{s.command}</span>
        </p>
      </div>
    </>
  );
}
