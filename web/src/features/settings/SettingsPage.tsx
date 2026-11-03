import { useSearchParams } from "@solidjs/router";
import { createEffect, useContext } from "solid-js";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Tab, TabList, TabPanel, Tabs } from "../../ui/Tabs";
import { NotificationsTab } from "./notifications/NotificationsTab";

const t = strings.settings;

/** The sections, in the URL as `?tab=`; more join as their features land (users, backups, …). */
const TABS = ["notifications"] as const;
type TabName = (typeof TABS)[number];

/** `/settings` (features/ui.md): the controller's settings in tabs named in the URL. */
export function SettingsPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const [params, setParams] = useSearchParams<{ tab?: string }>();
  const admin = () => session.user.role === "admin";
  const tab = (): TabName =>
    (TABS as readonly string[]).includes(params.tab ?? "")
      ? (params.tab as TabName)
      : "notifications";

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
        onChange={(value) => setParams({ tab: value === "notifications" ? undefined : value })}
        aria-label={strings.nav.settings.label}
      >
        <TabList>
          <Tab value="notifications">{t.tabs.notifications}</Tab>
        </TabList>
        <TabPanel value="notifications">
          <NotificationsTab nodes={live.fleet.nodes} admin={admin()} />
        </TabPanel>
      </Tabs>
    </div>
  );
}
