import {
  Camera,
  Cpu,
  Gamepad2,
  HardDrive,
  House,
  Laptop,
  Monitor,
  Printer,
  Router,
  Server,
  Smartphone,
  Speaker,
  Tablet,
  Tv,
  Watch,
} from "lucide";
import type { Client } from "../api/gen/Client";
import type { ClientKind } from "../api/gen/ClientKind";
import type { Connection } from "../api/gen/Connection";
import { strings } from "./strings";

/** The icon of each kind of client (lucide data, rendered by `ui/Icon`). */
export const kindIcons: Record<ClientKind, typeof Laptop> = {
  phone: Smartphone,
  tablet: Tablet,
  laptop: Laptop,
  desktop: Monitor,
  server: Server,
  watch: Watch,
  tv: Tv,
  speaker: Speaker,
  console: Gamepad2,
  printer: Printer,
  camera: Camera,
  storage: HardDrive,
  smart_home: House,
  network: Router,
  unknown: Cpu,
};

/** What people call a client: its alias, else its DHCP hostname, else its MAC address. */
export function clientName(client: Pick<Client, "alias" | "hostname" | "mac">): string {
  return client.alias ?? client.hostname ?? client.mac;
}

/** The node a client is (or was) at: an adopted one by name, or a mesh peer that is not adopted. */
export function upstreamName(
  connection: Pick<Connection, "node" | "upstream_name">,
  nodeName: (id: number) => string,
): string {
  if (connection.node != null) return nodeName(connection.node);
  return connection.upstream_name
    ? strings.clients.peer(connection.upstream_name)
    : strings.clients.unadopted;
}

/** How an experience score reads: good from 80, fair from 50, poor below. */
export function scoreTone(score: number | null | undefined): "ok" | "warn" | "crit" | "neutral" {
  if (score == null) return "neutral";
  if (score >= 80) return "ok";
  return score >= 50 ? "warn" : "crit";
}

/** How a Wi-Fi signal reads: good from −65 dBm, fair from −75 dBm, weak below. */
export function signalTone(dbm: number | null | undefined): "ok" | "warn" | "crit" | "neutral" {
  if (dbm == null) return "neutral";
  if (dbm >= -65) return "ok";
  return dbm >= -75 ? "warn" : "crit";
}
