import type { Band } from "../../api/gen/Band";
import type { ManagedPackage } from "../../api/gen/ManagedPackage";
import type { NodeId } from "../../api/gen/NodeId";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { WanConfig } from "../../api/gen/WanConfig";
import { isCountry } from "../../lib/countries";
import { cidr, formatIpv4, ipv4, isHost, isPrivate, subnetText } from "../../lib/ipv4";
import { strings } from "../../lib/strings";

const e = strings.setup.errors;

/**
 * The packages the wizard manages on the gateway: what a router needs to route. The feed
 * packages (SQM, failover, UPnP) wait for the device page, since installing them needs the
 * internet the wizard is only setting up.
 */
export const CORE_PACKAGES: readonly ManagedPackage[] = [
  "system",
  "network",
  "wireless",
  "dhcp",
  "firewall",
];

/** The WLAN the wizard makes. */
export const WLAN_ID = "home";
/** The DHCP pool's host numbers in the main network, a /24. */
const POOL = { start: 100, end: 249 } as const;

/** The address plan as typed: the site's space and the router's address in the main network. */
export interface AddressDraft {
  space: string;
  router: string;
}

export type AddressErrors = Partial<Record<keyof AddressDraft, string>>;

export function addressDraftOf(config: SiteConfig): AddressDraft {
  const main = config.networks.find((n) => n.purpose === "default");
  return {
    space: config.addressing.ipv4,
    router: main?.ipv4?.address.split("/")[0] ?? "",
  };
}

export function checkAddresses(draft: AddressDraft): AddressErrors {
  const errors: AddressErrors = {};
  const space = cidr(draft.space);
  if (!space) errors.space = e.space;
  else if (space.len < 8 || space.len > 24) errors.space = e.spaceSize;
  else if (!isPrivate(space.network)) errors.space = e.spacePrivate;
  const router = ipv4(draft.router.trim());
  if (router === undefined) {
    errors.router = e.router;
  } else if (space && !errors.space) {
    const host = router % 256;
    if (router < space.network || router > space.broadcast) {
      errors.router = e.routerOutside(subnetText(space));
    } else if (host === 0 || host === 255) {
      errors.router = e.routerHost;
    } else if (host >= POOL.start && host <= POOL.end) {
      errors.router = e.routerInPool(POOL.start, POOL.end);
    }
  }
  return errors;
}

/** The main network of a checked draft: a /24 around the router, with its DHCP pool. */
export function mainNetwork(draft: AddressDraft): {
  space: string;
  address: string;
  subnet: string;
  start: string;
  end: string;
} {
  const router = ipv4(draft.router.trim()) ?? 0;
  const base = router - (router % 256);
  const space = cidr(draft.space);
  return {
    space: space ? subnetText(space) : draft.space.trim(),
    address: `${formatIpv4(router)}/24`,
    subnet: `${formatIpv4(base)}/24`,
    start: formatIpv4(base + POOL.start),
    end: formatIpv4(base + POOL.end),
  };
}

/** Whether a host at `from` has to move to reach the main network `draft` describes. */
export function moves(from: string, draft: AddressDraft): boolean {
  const now = ipv4(from);
  const lan = cidr(mainNetwork(draft).address);
  return now === undefined || !lan || !isHost(lan, now);
}

/** The first Wi-Fi network as typed. */
export interface WifiDraft {
  enabled: boolean;
  ssid: string;
  passphrase: string;
  country: string;
}

export type WifiErrors = Partial<Record<"ssid" | "passphrase" | "country", string>>;

export function checkWifi(draft: WifiDraft): WifiErrors {
  if (!draft.enabled) return {};
  const errors: WifiErrors = {};
  const bytes = new TextEncoder().encode(draft.ssid).length;
  if (draft.ssid.trim() === "" || bytes > 32) errors.ssid = e.ssid;
  if (draft.passphrase.length < 8 || draft.passphrase.length > 63) errors.passphrase = e.passphrase;
  if (!isCountry(draft.country)) errors.country = e.country;
  return errors;
}

/**
 * The bands the wizard's WLAN goes on: the router's 2.4 and 5 GHz radios. 6 GHz needs WPA3
 * alone, which older devices can't join, so it waits for the Wi-Fi editor.
 */
export function wlanBands(radios: readonly Band[]): Band[] {
  return (["2g", "5g"] as const).filter((band) => radios.includes(band));
}

/** What the wizard chose. */
export interface SetupChoices {
  gateway: NodeId;
  wan: WanConfig;
  addresses: AddressDraft;
  /** Nothing when Wi-Fi is left alone. */
  wifi: WifiDraft | undefined;
  bands: readonly Band[];
  /** The browser's time zone, when the site should take it. */
  timezone: string | undefined;
}

/** The site with the wizard's choices: one document, saved as one revision. */
export function withSetup(config: SiteConfig, choices: SetupChoices): SiteConfig {
  const lan = mainNetwork(choices.addresses);
  const inLan = cidr(lan.address);
  const main = config.networks.find((n) => n.purpose === "default");
  const networks = config.networks.map((network) =>
    network !== main || !network.ipv4
      ? network
      : {
          ...network,
          ipv4: {
            ...network.ipv4,
            address: lan.address,
            dhcp: network.ipv4.dhcp && {
              ...network.ipv4.dhcp,
              start: lan.start,
              end: lan.end,
              reservations: network.ipv4.dhcp.reservations.filter((r) => {
                const ip = ipv4(r.ip);
                return ip !== undefined && inLan !== undefined && isHost(inLan, ip);
              }),
            },
          },
        },
  );
  const wifi = choices.wifi?.enabled ? choices.wifi : undefined;
  const known = config.wlans.find((wlan) => wlan.id === WLAN_ID);
  const wlans = !wifi
    ? config.wlans
    : [
        ...config.wlans.filter((wlan) => wlan.id !== WLAN_ID),
        {
          id: WLAN_ID,
          ssid: wifi.ssid,
          enabled: true,
          network: main?.id ?? "lan",
          security: "wpa2_wpa3" as const,
          // The save sets the secret; its reference is the server's to make.
          passphrase: known?.passphrase ?? null,
          bands: [...choices.bands],
          nodes: { scope: "all" as const },
          hidden: false,
          isolation: false,
          max_clients: null,
          schedule: null,
          roaming: known?.roaming ?? { fast: null, hints: true },
        },
      ];
  const node = config.nodes.find((n) => n.node === choices.gateway);
  const managed = [...new Set([...(node?.managed ?? []), ...CORE_PACKAGES])].sort();
  const nodes = node
    ? config.nodes.map((n) => (n === node ? { ...n, managed } : n))
    : [
        ...config.nodes,
        { node: choices.gateway, hostname: null, managed, radios: [], ports: [], raw: [] },
      ];
  return {
    ...config,
    site: {
      ...config.site,
      ...(wifi && { country: wifi.country }),
      ...(choices.timezone && { timezone: choices.timezone }),
    },
    addressing: { ...config.addressing, ipv4: lan.space },
    networks,
    wlans,
    nodes,
    wan: choices.wan,
  };
}

/** The secrets the wizard's save carries. */
export function setupSecrets(
  wan: Record<string, string> | undefined,
  wifi: WifiDraft | undefined,
): Record<string, string> | undefined {
  const secrets = {
    ...wan,
    ...(wifi?.enabled && { [`wlans/${WLAN_ID}/passphrase`]: wifi.passphrase }),
  };
  return Object.keys(secrets).length > 0 ? secrets : undefined;
}
