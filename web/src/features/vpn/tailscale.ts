import type { Network } from "../../api/gen/Network";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { Tailscale } from "../../api/gen/Tailscale";
import type { TailscaleStatus } from "../../api/gen/TailscaleStatus";
import { cidr, subnetText } from "../../lib/ipv4";
import { isHostname } from "../../lib/names";
import { strings } from "../../lib/strings";
import type { Tone } from "../../ui/Badge";

const t = strings.vpn.tailscale;
const e = t.errors;

/** The package's UDP port, which the WAN opens for direct connections (`PORT` in the model). */
export const PORT = 41641;
/** The zone of `tailscale0` (`ZONE` in the model). */
const ZONE = "tailscale";
/** Where the auth key sits in the document. */
export const AUTH_KEY = "vpn/tailscale/auth_key";
/** Where the tailnet's admin approves subnet routes and exit nodes. */
export const ADMIN_CONSOLE = "https://login.tailscale.com/admin/machines";
/** The route a gateway offered as an exit node advertises. */
const EXIT_ROUTE = "0.0.0.0/0";

/** An auth key as the model takes it (`is_auth_key`): `tskey-` and letters, digits, - and _. */
export function isAuthKey(key: string): boolean {
  return /^tskey-[A-Za-z0-9_-]{1,250}$/.test(key);
}

/** The networks Tailscale can advertise: those with an IPv4 subnet. */
export function routable(networks: readonly Network[]): Network[] {
  return networks.filter((network) => network.ipv4 !== null);
}

/** A network's IPv4 subnet, such as `10.0.30.0/24`, or empty without one. */
export function subnetOf(network: Network): string {
  const subnet = cidr(network.ipv4?.address ?? "");
  return subnet ? subnetText(subnet) : "";
}

/** How the gateway logs in: through the URL an admin opens, or by itself with an auth key. */
export type Login = "url" | "key";

export interface TailscaleDraft {
  enabled: boolean;
  /** The advertised networks, by id. */
  routes: string[];
  exitNode: boolean;
  hostname: string;
  login: Login;
  /** A new auth key; empty keeps the saved one. */
  authKey: string;
  /** The site holds an auth key already. */
  keySaved: boolean;
}

export type TailscaleErrors = Partial<Record<"routes" | "hostname" | "authKey" | "port", string>>;

/** The editor's draft of `current`; a new one advertises the default networks. */
export function tailscaleDraft(current: Tailscale | null, config: SiteConfig): TailscaleDraft {
  const networks = routable(config.networks);
  if (!current) {
    return {
      enabled: true,
      routes: networks.filter((n) => n.purpose === "default").map((n) => n.id),
      exitNode: false,
      hostname: "",
      login: "url",
      authKey: "",
      keySaved: false,
    };
  }
  const ids = new Set(networks.map((n) => n.id));
  return {
    enabled: current.enabled,
    routes: current.routes.filter((id) => ids.has(id)),
    exitNode: current.exit_node,
    hostname: current.hostname ?? "",
    login: current.auth_key ? "key" : "url",
    authKey: "",
    keySaved: current.auth_key !== null,
  };
}

/** What a save sends: the settings and the auth key's change, if any. */
export interface TailscaleEdit {
  tailscale: Tailscale;
  secrets?: Record<string, string | null>;
}

/** The draft checked as the model checks it (`validate/tailscale.rs`), or its errors. */
export function tailscaleOf(
  draft: TailscaleDraft,
  current: Tailscale | null,
  config: SiteConfig,
): { edit?: TailscaleEdit; errors: TailscaleErrors } {
  const errors: TailscaleErrors = {};
  const hostname = draft.hostname.trim();
  if (hostname !== "" && !isHostname(hostname)) errors.hostname = e.hostname;
  let secrets: Record<string, string | null> | undefined;
  const key = draft.authKey.trim();
  if (draft.login === "key") {
    if (key !== "") {
      if (isAuthKey(key)) secrets = { [AUTH_KEY]: key };
      else errors.authKey = e.authKey;
    } else if (!draft.keySaved) {
      errors.authKey = e.authKeyMissing;
    }
  } else if (draft.keySaved) {
    secrets = { [AUTH_KEY]: null };
  }
  if (draft.enabled) {
    const forward = config.firewall.forwards.find(
      (f) => f.enabled && f.proto !== "tcp" && f.ports.first <= PORT && PORT <= f.ports.last,
    );
    if (forward) errors.port = e.forwarded(forward.name);
    const wg = config.vpn.wireguard;
    if (wg?.enabled && wg.port === PORT) errors.port ??= e.wireguard;
  }
  if (Object.keys(errors).length > 0) return { errors };
  const chosen = new Set(draft.routes);
  const tailscale: Tailscale = {
    enabled: draft.enabled,
    // In the networks' order, as the document lists them.
    routes: routable(config.networks)
      .filter((n) => chosen.has(n.id))
      .map((n) => n.id),
    exit_node: draft.exitNode,
    hostname: hostname === "" ? null : hostname,
    auth_key: current?.auth_key ?? null,
  };
  return { edit: { tailscale, ...(secrets && { secrets }) }, errors };
}

/**
 * The config with Tailscale saved, or removed (`null`): the zone's policies go with it, as the
 * model knows the zone only while Tailscale is set up.
 */
export function withTailscale(config: SiteConfig, tailscale: Tailscale | null): SiteConfig {
  const firewall = tailscale
    ? config.firewall
    : {
        ...config.firewall,
        policies: config.firewall.policies.filter((p) => p.from !== ZONE && p.to !== ZONE),
      };
  return { ...config, firewall, vpn: { ...config.vpn, tailscale } };
}

/** Tailscale's issues on `vpn/tailscale/…` as the editor's errors; the rest as text. */
export function tailscaleErrorsOfIssues(issues: readonly { path: string; message: string }[]): {
  errors: TailscaleErrors;
  other: string[];
} {
  const errors: TailscaleErrors = {};
  const other: string[] = [];
  for (const issue of issues) {
    const path = issue.path;
    if (path === "vpn/tailscale/hostname") errors.hostname ??= issue.message;
    else if (path === AUTH_KEY) errors.authKey ??= issue.message;
    else if (path.startsWith("vpn/tailscale/routes/")) errors.routes ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/** The daemon's state for people, and which of login and logout make sense in it. */
export interface DaemonState {
  tone: Tone;
  text: string;
  login: boolean;
  logout: boolean;
}

/** Tailscale's state on the gateway, from the site's settings and what the gateway reports. */
export function tailscaleState(
  tailscale: Tailscale,
  status: TailscaleStatus | undefined,
): DaemonState {
  const s = t.status;
  const only = (tone: Tone, text: string): DaemonState => ({
    tone,
    text,
    login: false,
    logout: false,
  });
  if (!tailscale.enabled) return only("neutral", s.off);
  if (!status || status.collected_at === null) return only("neutral", s.unknown);
  if (!status.online) return only("warn", s.offline);
  const daemon = status.daemon;
  if (!daemon) return only("info", s.pending);
  const loggedOut = daemon.prefs?.logged_out ?? daemon.state === "needs_login";
  const state = (tone: Tone, text: string): DaemonState => ({
    tone,
    text,
    login: loggedOut,
    logout: !loggedOut,
  });
  switch (daemon.state) {
    case null:
      return only("warn", s.down);
    case "needs_login":
      return state("warn", tailscale.auth_key ? s.needsLoginKey : s.needsLogin);
    case "needs_machine_auth":
      return state("warn", s.approval);
    case "stopped":
      return state("info", loggedOut ? s.needsLogin : s.stopped);
    case "running":
      return state(daemon.online ? "ok" : "warn", daemon.online ? s.running : s.unreachable);
    default:
      return state("info", s.starting);
  }
}

/** The routes awaiting approval, for people: subnets, and the exit node by name. */
export function unapprovedText(routes: readonly string[]): string {
  return routes.map((route) => (route === EXIT_ROUTE ? t.exitRoute : route)).join(", ");
}
