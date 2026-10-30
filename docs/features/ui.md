# UI/UX & design system

Read this when building any page, component or visualization.
Milestones: M0.3 (skeleton), M1.8 (app shell and design system v1), every feature WP, M9.1 (polish).

## Goals

Professional, modern and fast: dense but calm. It should feel like a premium product, with Linear/Vercel-level polish and UniFi-level network clarity. Excellent on desktop and genuinely useful on a phone.

## Stack (D-002, D-018, D-045)

- SolidJS 2.0 release candidate with `@solidjs/web` (D-045); move to GA releases as they age in
- Vite, strict TypeScript
- Tailwind CSS v4 with design tokens as CSS variables
- Kobalte 2 (accessible headless primitives; patched, D-045), @solidjs/router 2 (config routes, plain `<a>` links)
- @tanstack/solid-query 6 for server state, plus WebSocket deltas
- Lucide icons through `ui/Icon` (the framework-free `lucide` data)
- uPlot for time series; d3-force (in a Web Worker) with Canvas for the topology
- @tanstack/virtual-core, wrapped in `ui/`, for large tables
- xterm.js and CodeMirror 6, lazy-loaded
- Biome; Vitest with Testing Library; Playwright e2e tests against demo mode

## Visual language

- **Themes:** dark-first plus a light theme; follows the system setting, with a toggle.
- **Token implementation:** `web/src/ui/tokens.css` defines each color once with `light-dark()`; `<html data-skin>` re-binds the tokens for the glass and UniFi-familiar skins (surfaces, borders, text, accent, `rounded-card`/`rounded-control`, `shadow-card`, `--blur-panel`, `h-row`, `w-sidebar`), so components never branch on the skin; the `glass:` variant adds glass-only effects. `tokens.test.ts` checks the contrast promises below for every skin and theme. `<html data-theme>` forces a theme, saved per browser and applied before the first paint by `public/theme-init.js`. Text tiers and the accent meet WCAG AA (4.5:1) on every surface; status, band and link colors meet 3:1 and are always paired with text or an icon.
- **Tokens:** surface levels, borders, text tiers, accent, and status (ok/warn/crit/info). Two token families are used identically in every chart, graph and legend:
  - **band colors:** 2.4 GHz amber, 5 GHz cyan, 6 GHz violet
  - **link-quality ramp**
- **Type:** Inter for the UI and JetBrains Mono for MACs, IPs and values, with tabular numbers.
- **Motion:** subtle (150–250 ms) and meaningful (state changes, live flows). `prefers-reduced-motion` is respected.
- **Density modes:** comfortable and compact.
- **Skins (D-046):** Graphite pro (default), Midnight glass and UniFi-familiar, each with light and dark. Skin and theme are separate choices, saved per browser and applied before the first paint by `public/theme-init.js`.

## Information architecture

- **Sidebar:** Dashboard · Topology · Devices · Clients · Wi-Fi · Networks · Internet · Security · Insights (Logs, Events, Metrics, RF, Floor plans) · Updates · Settings.
- **Global:**
  - top bar: site health pill, search, pending changes, notifications, user menu
  - **⌘K command palette:** navigate, find any client/device/MAC/IP, run actions ("reboot AP2", "optimize channels")
  - keyboard shortcuts, toasts, breadcrumbs
- **Device page tabs:** Overview · Radios · Ports · Clients · Mesh · Config · Logs · Tools · Terminal.
- **Client page tabs:** Overview · Timeline · Signal · Traffic · Doctor.

## Interaction patterns

- **Staged changes:** edits collect in a Pending changes tray. **Review** shows a UCI diff per node and an impact summary ("Wi-Fi restarts on 3 nodes, about 5 s"). **Apply** shows live job progress. Any revision can be rolled back from history.
- Everything is live through WebSocket deltas; there are no refresh buttons. Skeleton loaders while loading. Optimistic UI only for harmless operations.
- Empty states give guided next steps, and every alert links to a fix.
- First-run, ISP, adoption, mesh and RF optimization are wizards, each ending in a review step.
- Forms validate client-side using the generated types and show the server's validation messages.

## Signature visualizations

1. Live topology: tree, RF and clients views, flow animation, replay slider
2. Floor-plan coverage heatmap
3. Spectrum and channel occupancy per band, our radios highlighted, planner before/after
4. Roaming timeline: swimlanes per AP showing client paths
5. WAN quality: smokeping-style latency bands with outage ribbons
6. Mesh link-quality matrix heatmap and path tree
7. Rolling-upgrade pipeline view
8. Log histogram by severity, with correlated events
9. Experience score donuts and distribution
10. Traffic: top clients (bars or treemap) and per-network usage over time

## Performance budgets

- Initial JS ≤ 150 KB gzip; routes lazy-loaded; Lighthouse score ≥ 95.
- 60 fps topology with 300 entities; tables virtualized above 200 rows.
- CSS containment to avoid layout thrash; WebSocket deltas applied in batches once per animation frame.

## Accessibility & i18n

WCAG 2.2 AA; full keyboard operation and focus management. Color is never the only signal (icons and patterns too). ARIA comes from Kobalte. Strings are centralized from day one, so i18n is ready (English first).

## Frontend structure

`web/src/{app, ui, lib, api/gen, features/<area>, viz/<kind>}`. Features never import other features; shared code goes in `ui/` (primitives) or `lib/` (API client, WebSocket client, formatters, appearance).

- **Primitives (`ui/`):** Icon, Button, Card and CardHeader, Badge and StatusDot, Stat, Sparkline, Table (virtualized above 200 rows), Tabs, Dialog and Drawer, Tooltip, Toast (`showToast`), Kbd. Interactive ones wrap Kobalte.
- **Shell (`app/`):** the sidebar (an icon rail in UniFi-familiar, a drawer on phones), the top bar (title, search, health), the Appearance menu (skin and theme), the ⌘K palette (`app/commands.ts`; features add their commands there) and a page error boundary.
- **Signed-out screens (`features/auth`):** first-run setup, "setup closed" and login; `app/App.tsx` shows them until there is a session, then the shell (D-047).
- **Live data (`lib/`):** `api.ts` (typed routes; the CSRF header on every POST; a 401 ends the session), `ws.ts` (`LiveSocket`: topics, reconnect, a flush per animation frame), `live.ts` (`LiveContext`: the fleet store; `createNodeView` for `node:<id>`), `session.ts` (`SessionContext`: the user, sign-out), `format.ts`.
- **Devices (`features/devices`):** the list (`/devices`) and a node's live overview (`/devices/:id`: status and reasons, stats with sparklines, radios, details, Wi-Fi clients); ⌘K opens a device found by name, MAC, address or model.
- **Gallery:** `/dev/ui` in development builds shows every primitive; `?skin=glass&theme=dark` applies a skin and theme, for screenshots.
