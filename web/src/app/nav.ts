import {
  Activity,
  BellRing,
  ChartLine,
  CircleArrowUp,
  Globe,
  Laptop,
  Layers,
  LayoutDashboard,
  Map as MapIcon,
  Network,
  RadioTower,
  Router,
  ScrollText,
  Settings,
  ShieldCheck,
  Wifi,
} from "lucide";
import { strings } from "../lib/strings";
import type { IconNode } from "../ui/Icon";

export interface NavItem {
  readonly path: string;
  readonly label: string;
  readonly description: string;
  readonly icon: IconNode;
}

export interface NavGroup {
  readonly label: string;
  readonly icon: IconNode;
  readonly items: readonly NavItem[];
}

export type NavEntry = NavItem | NavGroup;

const s = strings.nav;

/** The sidebar, in the order of the information architecture (ui.md). */
export const nav: readonly NavEntry[] = [
  { path: "/", ...s.dashboard, icon: LayoutDashboard },
  { path: "/topology", ...s.topology, icon: Network },
  { path: "/devices", ...s.devices, icon: Router },
  { path: "/clients", ...s.clients, icon: Laptop },
  { path: "/wifi", ...s.wifi, icon: Wifi },
  { path: "/networks", ...s.networks, icon: Layers },
  { path: "/internet", ...s.internet, icon: Globe },
  { path: "/security", ...s.security, icon: ShieldCheck },
  {
    ...s.insights,
    icon: ChartLine,
    items: [
      { path: "/insights/logs", ...s.logs, icon: ScrollText },
      { path: "/insights/events", ...s.events, icon: BellRing },
      { path: "/insights/metrics", ...s.metrics, icon: Activity },
      { path: "/insights/rf", ...s.rf, icon: RadioTower },
      { path: "/insights/floor-plans", ...s.floorPlans, icon: MapIcon },
    ],
  },
  { path: "/updates", ...s.updates, icon: CircleArrowUp },
  { path: "/settings", ...s.settings, icon: Settings },
];

export function isGroup(entry: NavEntry): entry is NavGroup {
  return "items" in entry;
}

/** Every page in the sidebar, groups flattened. */
export const navItems: readonly NavItem[] = nav.flatMap((entry) =>
  isGroup(entry) ? entry.items : [entry],
);

export function findNavItem(pathname: string): NavItem | undefined {
  const path = pathname.length > 1 ? pathname.replace(/\/+$/, "") : pathname;
  return navItems.find((item) => item.path === path);
}
