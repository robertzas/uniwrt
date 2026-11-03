import { createRouter, type RouteDefinition, type RouterHistory } from "@solidjs/router";
import { lazy } from "solid-js";
import { NotFound } from "./NotFound";
import { navItems } from "./nav";
import { SectionPage } from "./SectionPage";

/**
 * One route per sidebar page. Each feature replaces its placeholder with a lazy-loaded page
 * (`lazy(() => import(...))`), which keeps the initial bundle within the budget in ui.md.
 */
const pages: Record<string, RouteDefinition["component"]> = {
  "/": lazy(() =>
    import("../features/dashboard/DashboardPage").then((m) => ({ default: m.DashboardPage })),
  ),
  "/topology": lazy(() =>
    import("../features/topology/TopologyPage").then((m) => ({ default: m.TopologyPage })),
  ),
  "/devices": lazy(() =>
    import("../features/devices/DevicesPage").then((m) => ({ default: m.DevicesPage })),
  ),
  "/clients": lazy(() =>
    import("../features/clients/ClientsPage").then((m) => ({ default: m.ClientsPage })),
  ),
  "/insights/logs": lazy(() =>
    import("../features/logs/LogsPage").then((m) => ({ default: m.LogsPage })),
  ),
  "/insights/events": lazy(() =>
    import("../features/events/EventsPage").then((m) => ({ default: m.EventsPage })),
  ),
  "/insights/alerts": lazy(() =>
    import("../features/alerts/AlertsPage").then((m) => ({ default: m.AlertsPage })),
  ),
  "/insights/metrics": lazy(() =>
    import("../features/metrics/MetricsPage").then((m) => ({ default: m.MetricsPage })),
  ),
  "/insights/rf": lazy(() => import("../features/rf/RfPage").then((m) => ({ default: m.RfPage }))),
  "/settings": lazy(() =>
    import("../features/settings/SettingsPage").then((m) => ({ default: m.SettingsPage })),
  ),
};

export const routes: RouteDefinition[] = [
  ...navItems.map((item) => ({
    path: item.path,
    component: pages[item.path] ?? (() => <SectionPage item={item} />),
  })),
  {
    path: "/devices/:id",
    component: lazy(() =>
      import("../features/devices/DevicePage").then((m) => ({ default: m.DevicePage })),
    ),
  },
  {
    path: "/clients/:mac",
    component: lazy(() =>
      import("../features/clients/ClientPage").then((m) => ({ default: m.ClientPage })),
    ),
  },
  // The design-system gallery, in development builds only (dead code in production).
  ...(import.meta.env.DEV
    ? [
        {
          path: "/dev/ui",
          component: lazy(() => import("../dev/Gallery").then((m) => ({ default: m.Gallery }))),
        },
      ]
    : []),
  { path: "*404", component: NotFound },
];

/** The app's router; tests pass a memory history. */
export function createAppRouter(history?: RouterHistory) {
  return createRouter({ routes, ...(history && { history }) });
}
