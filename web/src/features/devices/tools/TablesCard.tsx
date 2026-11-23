import { createMemo, createSignal, useContext } from "solid-js";
import type { NodeId } from "../../../api/gen/NodeId";
import { createClientsView } from "../../../lib/clientList";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Card, CardHeader } from "../../../ui/Card";
import { Tab, TabList, TabPanel, Tabs } from "../../../ui/Tabs";
import { ConntrackPanel } from "./ConntrackPanel";
import { NeighborsPanel } from "./NeighborsPanel";
import { RoutesPanel } from "./RoutesPanel";
import { indexClients } from "./tables";

const t = strings.device.tools.tables;

type Kind = keyof typeof t.tabs;

/**
 * The device's neighbor, route and conntrack tables (D-142), each read from the device when its
 * tab opens and again on *Refresh*; sortable, filterable, and linked to the clients they show.
 */
export function TablesCard(props: { node: NodeId }) {
  const live = useContext(LiveContext);
  const clients = createClientsView(live);
  const index = createMemo(() => indexClients(clients.clients));
  const [tab, setTab] = createSignal<Kind>("neighbors");
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Tabs value={tab()} onChange={(value) => setTab(value as Kind)} aria-label={t.title}>
        <TabList class="mx-4">
          <Tab value="neighbors">{t.tabs.neighbors}</Tab>
          <Tab value="routes">{t.tabs.routes}</Tab>
          <Tab value="conntrack">{t.tabs.conntrack}</Tab>
        </TabList>
        <TabPanel value="neighbors" class="pb-2">
          <NeighborsPanel node={props.node} clients={index()} />
        </TabPanel>
        <TabPanel value="routes" class="pb-2">
          <RoutesPanel node={props.node} />
        </TabPanel>
        <TabPanel value="conntrack" class="pb-2">
          <ConntrackPanel node={props.node} clients={index()} />
        </TabPanel>
      </Tabs>
    </Card>
  );
}
