import { For, Show } from "solid-js";
import { strings } from "../../lib/strings";
import { Checkbox } from "../../ui/Checkbox";
import { FormField, INPUT } from "../../ui/FormField";
import { TextField } from "../../ui/TextField";
import { type DraftErrors, LEASES, type NetworkDraft } from "./draft";

const t = strings.networks.editor;

/** The editor's IPv4 side: the gateway's address, the pool, its options and network boot. */
export function Ipv4Fields(props: {
  draft: NetworkDraft;
  errors: DraftErrors;
  /** The site's domain, which an empty domain hands out. */
  domain: string;
  change: (next: Partial<NetworkDraft>) => void;
}) {
  const d = () => props.draft;
  const leases = () =>
    LEASES.map(String).includes(d().lease)
      ? LEASES.map(String)
      : [...LEASES.map(String), d().lease];
  return (
    <section class="flex flex-col gap-3" aria-label={t.ipv4}>
      <h3 class="font-semibold text-sm">{t.ipv4}</h3>
      <Checkbox checked={d().ipv4} onChange={(ipv4) => props.change({ ipv4 })} label={t.ipv4On} />
      <Show when={d().ipv4}>
        <TextField
          label={t.address}
          value={d().address}
          onInput={(address) => props.change({ address })}
          hint={t.addressHint}
          error={props.errors.address}
        />
        <Checkbox checked={d().dhcp} onChange={(dhcp) => props.change({ dhcp })} label={t.dhcpOn} />
        <Show when={d().dhcp}>
          <div class="grid gap-3 sm:grid-cols-3">
            <TextField
              label={t.start}
              value={d().start}
              onInput={(start) => props.change({ start })}
              error={props.errors.start}
            />
            <TextField
              label={t.end}
              value={d().end}
              onInput={(end) => props.change({ end })}
              error={props.errors.end}
            />
            <FormField label={t.lease} for="network-lease" error={props.errors.lease}>
              <select
                id="network-lease"
                class={[INPUT, "border-edge"]}
                value={d().lease}
                onChange={(event) => props.change({ lease: event.currentTarget.value })}
              >
                <For each={leases()}>
                  {(seconds) => (
                    <option value={seconds}>
                      {t.leases[seconds] ?? t.leaseOther(Number(seconds))}
                    </option>
                  )}
                </For>
              </select>
            </FormField>
          </div>
          <h4 class="mt-1 font-medium text-fg-muted text-xs uppercase tracking-wide">
            {t.options}
          </h4>
          <div class="grid gap-3 sm:grid-cols-2">
            <TextField
              label={t.dns}
              value={d().dns}
              onInput={(dns) => props.change({ dns })}
              hint={t.dnsHint}
              error={props.errors.dns}
            />
            <TextField
              label={t.ntp}
              value={d().ntp}
              onInput={(ntp) => props.change({ ntp })}
              hint={t.ntpHint}
              error={props.errors.ntp}
            />
            <TextField
              label={t.domain}
              value={d().domain}
              onInput={(domain) => props.change({ domain })}
              hint={t.domainHint(props.domain)}
              error={props.errors.domain}
            />
          </div>
          <FormField
            label={t.custom}
            for="network-custom"
            hint={t.customHint}
            error={props.errors.custom}
          >
            <textarea
              id="network-custom"
              rows={2}
              class={[
                INPUT,
                "h-auto py-2 font-mono",
                props.errors.custom ? "border-crit" : "border-edge",
              ]}
              value={d().custom}
              onInput={(event) => props.change({ custom: event.currentTarget.value })}
            />
          </FormField>
          <Checkbox checked={d().boot} onChange={(boot) => props.change({ boot })} label={t.boot} />
          <Show when={d().boot}>
            <div class="grid gap-3 sm:grid-cols-3">
              <TextField
                label={t.bootFile}
                value={d().bootFile}
                onInput={(bootFile) => props.change({ bootFile })}
                hint={t.bootFileHint}
                error={props.errors.bootFile}
              />
              <TextField
                label={t.bootName}
                value={d().bootName}
                onInput={(bootName) => props.change({ bootName })}
                error={props.errors.bootName}
              />
              <TextField
                label={t.bootServer}
                value={d().bootServer}
                onInput={(bootServer) => props.change({ bootServer })}
                hint={t.bootServerHint}
                error={props.errors.bootServer}
              />
            </div>
          </Show>
        </Show>
      </Show>
    </section>
  );
}
