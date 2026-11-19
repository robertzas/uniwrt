import type { Client } from "../../api/gen/Client";
import type { Issue } from "../../api/gen/Issue";
import type { Network } from "../../api/gen/Network";
import type { Wlan } from "../../api/gen/Wlan";
import { strings } from "../../lib/strings";

const t = strings.wifi;

/** The clients online on a WLAN, by its SSID. */
export function clientsOn(wlan: Wlan, clients: readonly Client[]): number {
  return clients.filter((client) => client.online && client.connection?.ssid === wlan.ssid).length;
}

/** The document's warnings about a WLAN. */
export function warningsOf(issues: readonly Issue[], id: string): string[] {
  return issues
    .filter((issue) => issue.path === `wlans/${id}` || issue.path.startsWith(`wlans/${id}/`))
    .map((issue) => issue.message);
}

/** Which nodes a WLAN is on, in words. */
export function scopeText(wlan: Wlan, nodeName: (id: number) => string): string {
  const scope = wlan.nodes;
  if (scope.scope === "all") return t.scope.all;
  const names = scope.nodes.map(nodeName).join(", ");
  return scope.scope === "only" ? t.scope.only(names) : t.scope.except(names);
}

/**
 * Why `text` can't be the mesh's wired backhaul VLAN (D-094), if it can't: 1–4094 and no
 * network's VLAN (the server's `mesh/wired_vlan` check).
 */
export function wiredVlanError(text: string, networks: readonly Network[]): string | undefined {
  const vid = Number(text.trim());
  if (text.trim() === "" || !Number.isInteger(vid) || vid < 1 || vid > 4094) {
    return t.mesh.vlanError;
  }
  const taken = networks.find((n) => n.vlan === vid);
  return taken ? t.mesh.vlanTaken(taken.name) : undefined;
}
