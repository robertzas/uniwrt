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
  "/devices": lazy(() =>
    import("../features/devices/DevicesPage").then((m) => ({ default: m.DevicesPage })),
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
