import { useSearchParams } from "@solidjs/router";
import { Plus, RotateCcw, Terminal } from "lucide";
import { createSignal, onSettled } from "solid-js";
import { isSkin, isThemePreference, setCurrentSkin, setThemePreference } from "../lib/appearance";
import { Badge, StatusDot } from "../ui/Badge";
import { Button } from "../ui/Button";
import { Card, CardHeader } from "../ui/Card";
import { Dialog } from "../ui/Dialog";
import { Sparkline } from "../ui/Sparkline";
import { Stat } from "../ui/Stat";
import { type Column, Table } from "../ui/Table";
import { Tab, TabList, TabPanel, Tabs } from "../ui/Tabs";
import { showToast } from "../ui/Toast";

/*
 * The design system on one page, for development only (routes.tsx registers it in dev builds).
 * `?skin=glass&theme=dark` applies a skin and theme, for screenshots. The data is made up.
 */

interface SampleNode {
  name: string;
  role: string;
  ip: string;
  clients: number;
  status: "ok" | "warn";
}

const nodes: SampleNode[] = [
  { name: "gw-home", role: "Gateway", ip: "192.168.1.1", clients: 6, status: "ok" },
  { name: "ap-living", role: "Mesh AP", ip: "192.168.1.2", clients: 11, status: "ok" },
  { name: "ap-upstairs", role: "Mesh AP", ip: "192.168.1.3", clients: 14, status: "ok" },
  { name: "ap-garage", role: "Wired AP", ip: "192.168.1.4", clients: 2, status: "warn" },
];

const columns: Column<SampleNode>[] = [
  {
    id: "name",
    header: "Name",
    cell: (n) => (
      <span class="flex items-center gap-2 font-medium">
        <StatusDot tone={n.status} />
        {n.name}
      </span>
    ),
  },
  { id: "role", header: "Role", cell: (n) => <span class="text-fg-muted">{n.role}</span> },
  { id: "ip", header: "Address", cell: (n) => <span class="font-mono text-xs">{n.ip}</span> },
  { id: "clients", header: "Clients", cell: (n) => n.clients, align: "end" },
];

const wave = (seed: number, base: number, amp: number) =>
  Array.from({ length: 30 }, (_, i) => base + amp * Math.sin(i / 3 + seed) + ((i * seed) % 7));

export function Gallery() {
  const [params] = useSearchParams();
  const [dialogOpen, setDialogOpen] = createSignal(false);
  onSettled(() => {
    const skin = String(params.skin ?? "");
    const theme = String(params.theme ?? "");
    if (isSkin(skin)) setCurrentSkin(skin);
    if (isThemePreference(theme)) setThemePreference(theme);
  });

  return (
    <div class="mx-auto flex max-w-6xl flex-col gap-5">
      <div class="flex flex-wrap items-center gap-3">
        <h1 class="font-semibold text-xl tracking-tight">ap-upstairs</h1>
        <Badge tone="ok">Online</Badge>
        <Badge tone="neutral">Observe mode</Badge>
        <div class="ml-auto flex gap-2">
          <Button icon={Terminal}>Terminal</Button>
          <Button icon={RotateCcw} onClick={() => setDialogOpen(true)}>
            Reboot
          </Button>
          <Button
            variant="primary"
            icon={Plus}
            onClick={() => showToast({ title: "Adoption started", tone: "ok" })}
          >
            Adopt
          </Button>
        </div>
      </div>
      <Tabs defaultValue="overview" aria-label="Device">
        <TabList>
          <Tab value="overview">Overview</Tab>
          <Tab value="radios">Radios</Tab>
          <Tab value="clients">Clients</Tab>
          <Tab value="logs">Logs</Tab>
        </TabList>
        <TabPanel value="overview" class="flex flex-col gap-4">
          <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
            <Stat label="Clients" value={14}>
              <span class="flex gap-3">
                <span class="flex items-center gap-1.5">
                  <span class="size-2 rounded-full bg-band-2g" />4
                </span>
                <span class="flex items-center gap-1.5">
                  <span class="size-2 rounded-full bg-band-5g" />8
                </span>
                <span class="flex items-center gap-1.5">
                  <span class="size-2 rounded-full bg-band-6g" />2
                </span>
              </span>
            </Stat>
            <Stat label="Throughput" value={78} unit="Mb/s">
              <Sparkline values={wave(1, 70, 30)} label="Throughput" class="text-accent" />
            </Stat>
            <Stat label="Mesh uplink" value={236} unit="/ 255 TQ">
              to gw-home · 5 GHz · −61 dBm
            </Stat>
            <Stat label="CPU" value={18} unit="%">
              <Sparkline values={wave(3, 18, 8)} label="CPU load" class="text-fg-subtle" />
            </Stat>
          </div>
          <Card>
            <CardHeader title="Devices" description="Sample data" actions={<span>4 nodes</span>} />
            <Table label="Devices" columns={columns} rows={nodes} rowKey={(n) => n.name} />
          </Card>
        </TabPanel>
        <TabPanel value="radios">Radios</TabPanel>
        <TabPanel value="clients">Clients</TabPanel>
        <TabPanel value="logs">Logs</TabPanel>
      </Tabs>
      <Dialog
        open={dialogOpen()}
        onOpenChange={setDialogOpen}
        title="Reboot ap-upstairs?"
        description="Its 14 clients roam to the other access points while it restarts, about 60 s."
      >
        <div class="flex justify-end gap-2 px-5 pt-2 pb-4">
          <Button variant="ghost" onClick={() => setDialogOpen(false)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => setDialogOpen(false)}>
            Reboot
          </Button>
        </div>
      </Dialog>
    </div>
  );
}
