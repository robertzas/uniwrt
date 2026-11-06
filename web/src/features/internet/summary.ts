import type { Job } from "../../api/gen/Job";
import type { Preset } from "../../api/gen/Preset";
import type { WanConfig } from "../../api/gen/WanConfig";
import { strings } from "../../lib/strings";

const t = strings.internet;

/** The connection card's rows: what the site's WAN is, in words. */
export function rowsOf(wan: WanConfig, presets: readonly Preset[]): [string, string][] {
  const preset = presets.find((p) => p.id === wan.preset);
  const p = wan.proto;
  const ipv4 = (() => {
    switch (p.kind) {
      case "pppoe":
        return t.pppoe(p.username);
      case "static":
        return t.staticAddress(p.address, p.gateway);
      default:
        return t.proto[p.kind];
    }
  })();
  const v6 = wan.ipv6;
  const ipv6 =
    v6.mode === "6rd"
      ? t.sixrd(v6.prefix, v6.border_relay)
      : v6.mode === "dhcpv6" && v6.reqprefix !== null
        ? `${t.ipv6Mode.dhcpv6} (/${v6.reqprefix})`
        : t.ipv6Mode[v6.mode];
  const vlan =
    wan.vlan === null
      ? t.untagged
      : wan.pcp !== null && wan.pcp > 0
        ? t.vlanPcp(wan.vlan, wan.pcp)
        : String(wan.vlan);
  return [
    [t.rows.preset, preset ? t.fromPreset(preset.name, preset.variant) : t.custom],
    [t.rows.ipv4, ipv4],
    [t.rows.vlan, vlan],
    [t.rows.ipv6, ipv6],
    [t.rows.mac, wan.mac ?? t.boardMac],
    [t.rows.mtu, wan.mtu === null ? t.auto : String(wan.mtu)],
  ];
}

/** What a finished WAN test says about IPv6: it worked, 6rd took over, or it failed. */
export type Ipv6Result = "ok" | "sixrd" | "failed" | "off";

export function ipv6Result(job: Job): Ipv6Result {
  const state = (name: string) => job.steps.find((step) => step.name === name)?.state;
  if (state("six_rd") === "done") return "sixrd";
  switch (state("ipv6")) {
    case "done":
      return "ok";
    case "failed":
      return "failed";
    default:
      return "off";
  }
}

/** Countries with presets, sorted by name, each with its name in English. */
export function countriesOf(presets: readonly Preset[]): { code: string; name: string }[] {
  const names = new Intl.DisplayNames(["en"], { type: "region" });
  const codes = [...new Set(presets.flatMap((p) => (p.country ? [p.country] : [])))];
  return codes
    .map((code) => ({ code, name: names.of(code) ?? code }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** The presets offered for `country`: its ISPs matching `search`, then the generic ones. */
export function offered(
  presets: readonly Preset[],
  country: string | null,
  search: string,
): { isps: Preset[]; generic: Preset[] } {
  const words = search.toLowerCase().split(/\s+/).filter(Boolean);
  const matches = (p: Preset) =>
    words.every((w) => `${p.name} ${p.variant} ${p.notes ?? ""}`.toLowerCase().includes(w));
  return {
    isps: presets.filter((p) => country !== null && p.country === country && matches(p)),
    generic: presets.filter((p) => p.country === null && matches(p)),
  };
}
