import type { Network } from "../../../api/gen/Network";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SshHardening } from "../../../api/gen/SshHardening";
import type { SshServerFacts } from "../../../api/gen/SshServerFacts";
import { managedOf, withManaged } from "./packages";
import { entryOf } from "./schedules";

/** The node's SSH hardening (security.md, Node hardening; D-150): none without an entry. */
export function sshOf(config: SiteConfig, node: number): SshHardening {
  return entryOf(config, node).ssh;
}

/**
 * `config` with the node's SSH hardening replaced. Any of it on manages `dropbear`, where render
 * writes it; turning it off keeps `dropbear` managed, so the apply undoes it on the device.
 */
export function withSsh(config: SiteConfig, node: number, ssh: SshHardening): SiteConfig {
  const on = ssh.key_only || ssh.management_only;
  const managed = managedOf(config, node);
  const next =
    on && !managed.includes("dropbear")
      ? withManaged(config, node, [...managed, "dropbear"])
      : config;
  const known = next.nodes.some((entry) => entry.node === node);
  const nodes = known ? next.nodes : [...next.nodes, entryOf(next, node)];
  return {
    ...next,
    nodes: nodes.map((entry) => (entry.node === node ? { ...entry, ssh } : entry)),
  };
}

/** The network the binding names: the management network, else the default one (render's). */
export function managementNetwork(config: SiteConfig): Network | undefined {
  return (
    config.networks.find((n) => n.purpose === "mgmt") ??
    config.networks.find((n) => n.purpose === "default")
  );
}

/** How the device's running SSH servers let people in, as its agent reports them. */
export interface SshNow {
  /** Whether a server takes passwords (for root). */
  passwords: boolean;
  /** The devices the servers are bound to; empty when one listens on every network. */
  devices: string[];
}

/** `null` where nothing is known or no server runs. */
export function sshNow(servers: readonly SshServerFacts[] | null | undefined): SshNow | null {
  const running = (servers ?? []).filter((server) => server.running);
  if (running.length === 0) return null;
  const open = running.some((server) => server.device === null);
  return {
    passwords: running.some((server) => server.password && server.root_password),
    devices: open ? [] : [...new Set(running.map((server) => server.device ?? ""))],
  };
}
