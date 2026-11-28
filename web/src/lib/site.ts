import type { Network } from "../api/gen/Network";
import type { NodeConfig } from "../api/gen/NodeConfig";
import type { SiteConfig } from "../api/gen/SiteConfig";
import type { SiteView } from "../api/gen/SiteView";
import { ApiError, api } from "./api";

/**
 * Saves the site's next revision: `edit` applied to the latest document, read just before, so
 * that the save builds on what another tab may have saved. Once another save wins the race
 * (409), it reads and edits again. A refused document throws its 422 with the issues.
 */
export async function saveSite(
  edit: (config: SiteConfig) => SiteConfig,
  message: string,
  secrets?: Record<string, string | null>,
): Promise<SiteView> {
  const once = async () => {
    const latest = await api.site.get();
    return api.site.save({
      base: latest.revision.id,
      message,
      config: edit(latest.config),
      ...(secrets && { secrets }),
    });
  };
  return once().catch((err) => {
    if (err instanceof ApiError && err.status === 409) return once();
    throw err;
  });
}

/** A node's entry with no overrides and nothing managed, as `NodeConfig::new` makes it. */
export function blankNode(node: number): NodeConfig {
  return {
    node,
    hostname: null,
    managed: [],
    radios: [],
    ports: [],
    raw: [],
    reboot: { kind: "site" },
    led_schedule: true,
    ssh: { key_only: false, management_only: false },
  };
}

/** `config` with the node's entry edited; a node without one gets a blank one first. */
export function withNode(
  config: SiteConfig,
  node: number,
  edit: (entry: NodeConfig) => NodeConfig,
): SiteConfig {
  const known = config.nodes.some((entry) => entry.node === node);
  const nodes = known ? config.nodes : [...config.nodes, blankNode(node)];
  return { ...config, nodes: nodes.map((entry) => (entry.node === node ? edit(entry) : entry)) };
}

/** The networks in the list's order: the default one first, then by VLAN, untagged ones last. */
export function sortNetworks(networks: readonly Network[]): Network[] {
  const rank = (n: Network) => (n.purpose === "default" ? -1 : (n.vlan ?? 5000));
  return [...networks].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}
