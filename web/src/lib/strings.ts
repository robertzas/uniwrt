/** Every user-facing string, kept in one place so the UI is ready for translation (ui.md). */
export const strings = {
  appName: "uniwrt",
  nav: {
    label: "Main",
    open: "Open navigation",
    close: "Close navigation",
    dashboard: {
      label: "Dashboard",
      description: "Network health, internet quality and recent events at a glance.",
    },
    topology: {
      label: "Topology",
      description: "A live map of the gateway, the mesh access points and every client.",
    },
    devices: {
      label: "Devices",
      description: "Adopt routers and access points, then manage each one or the whole fleet.",
    },
    clients: {
      label: "Clients",
      description: "Every connected client, with signal, roaming history and traffic.",
    },
    wifi: {
      label: "Wi-Fi",
      description: "Wireless networks, bands, roaming and per-device passwords.",
    },
    networks: {
      label: "Networks",
      description: "LAN, VLANs, DHCP and DNS for every network segment.",
    },
    internet: {
      label: "Internet",
      description: "WAN setup with ISP presets, connectivity tests and failover.",
    },
    security: {
      label: "Security",
      description: "Firewall zones, port forwards and node hardening.",
    },
    insights: { label: "Insights" },
    logs: {
      label: "Logs",
      description: "Search and live-tail the logs of every node, with explanations.",
    },
    events: {
      label: "Events",
      description: "Alerts and notable changes across the network.",
    },
    metrics: {
      label: "Metrics",
      description: "Throughput, latency and resource use over time.",
    },
    rf: {
      label: "RF",
      description: "Channel occupancy, interference and the channel planner.",
    },
    floorPlans: {
      label: "Floor plans",
      description: "Place access points on a floor plan and see the coverage.",
    },
    updates: {
      label: "Updates",
      description: "Firmware for every node, rolled out in safe waves.",
    },
    settings: {
      label: "Settings",
      description: "Controller settings, users, backups and certificates.",
    },
  },
  page: {
    soon: "Arrives in an upcoming release",
    notFound: {
      title: "Page not found",
      description:
        "There is nothing at this address. It may have moved, or the link is incomplete.",
      home: "Go to the dashboard",
    },
  },
  health: {
    checking: "Connecting",
    online: "Online",
    offline: "Unreachable",
    version: (version: string) => `v${version}`,
    details: (commit: string, builtAt: string) => `Commit ${commit}, built ${builtAt}`,
    unreachable: "The controller is not answering. Live data resumes when it is back.",
  },
  theme: {
    label: "Theme",
    system: "System",
    light: "Light",
    dark: "Dark",
  },
} as const;
