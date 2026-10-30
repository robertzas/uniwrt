import { createRouter, type RouteDefinition, type RouterHistory } from "@solidjs/router";
import { NotFound } from "./NotFound";
import { navItems } from "./nav";
import { SectionPage } from "./SectionPage";

/**
 * One route per sidebar page. Each feature replaces its placeholder with a lazy-loaded page
 * (`lazy(() => import(...))`), which keeps the initial bundle within the budget in ui.md.
 */
export const routes: RouteDefinition[] = [
  ...navItems.map((item) => ({ path: item.path, component: () => <SectionPage item={item} /> })),
  { path: "*404", component: NotFound },
];

/** The app's router; tests pass a memory history. */
export function createAppRouter(history?: RouterHistory) {
  return createRouter({ routes, ...(history && { history }) });
}
