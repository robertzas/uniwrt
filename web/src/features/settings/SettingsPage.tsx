import { useSearchParams } from "@solidjs/router";
import { createEffect, createMemo, useContext } from "solid-js";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Tab, TabList, TabPanel, Tabs } from "../../ui/Tabs";
import { BackupsTab } from "./backups/BackupsTab";
import { NotificationsTab } from "./notifications/NotificationsTab";
import { RevisionsTab } from "./revisions/RevisionsTab";
import { SiteTab } from "./site/SiteTab";
import { SupportTab } from "./support/SupportTab";
import { UsersTab } from "./users/UsersTab";

const t = strings.settings;

/** The sections, in the URL as `?tab=`. */
const TABS = ["site", "notifications", "revisions", "backups", "users", "support"] as const;
type TabName = (typeof TABS)[number];

/** `/settings` (features/ui.md): the controller's settings in tabs named in the URL. */
export function SettingsPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const [params, setParams] = useSearchParams<{ tab?: string }>();
  const admin = () => session.user.role === "admin";
  const tab = (): TabName =>
    (TABS as readonly string[]).includes(params.tab ?? "") ? (params.tab as TabName) : "site";
  const names = createMemo(() => new Map(live.fleet.nodes.map((n) => [n.id, n.name])));

  createEffect(
    () => strings.nav.settings.label,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-5xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{strings.nav.settings.label}</h1>
        <p class="mt-1 text-fg-muted text-sm">{strings.nav.settings.description}</p>
      </header>
      <Tabs
        value={tab()}
        onChange={(value) => setParams({ tab: value === "site" ? undefined : value })}
        aria-label={strings.nav.settings.label}
      >
        <TabList>
          <Tab value="site">{t.tabs.site}</Tab>
          <Tab value="notifications">{t.tabs.notifications}</Tab>
          <Tab value="revisions">{t.tabs.revisions}</Tab>
          <Tab value="backups">{t.tabs.backups}</Tab>
          <Tab value="users">{t.tabs.users}</Tab>
          <Tab value="support">{t.tabs.support}</Tab>
        </TabList>
        <TabPanel value="site">
          <SiteTab admin={admin()} />
        </TabPanel>
        <TabPanel value="notifications">
          <NotificationsTab nodes={live.fleet.nodes} admin={admin()} />
        </TabPanel>
        <TabPanel value="revisions">
          <RevisionsTab nodeName={(id) => names().get(id)} admin={admin()} />
        </TabPanel>
        <TabPanel value="backups">
          <BackupsTab admin={admin()} />
        </TabPanel>
        <TabPanel value="users">
          <UsersTab admin={admin()} />
        </TabPanel>
        <TabPanel value="support">
          <SupportTab admin={admin()} />
        </TabPanel>
      </Tabs>
    </div>
  );
}
