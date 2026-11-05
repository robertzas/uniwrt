import type { ApplyPlan } from "../api/gen/ApplyPlan";
import type { Job } from "../api/gen/Job";
import type { JobNode } from "../api/gen/JobNode";
import type { NodePlan } from "../api/gen/NodePlan";

/** A node's plan: by default an access point that changes nothing. */
export function nodePlan(node: number, name: string, extra: Partial<NodePlan> = {}): NodePlan {
  return {
    node,
    name,
    role: "ap",
    action: "unchanged",
    reason: null,
    packages: [],
    drift: [],
    raw: [],
    services: [],
    address: null,
    hops: 1,
    timeout_s: 120,
    warnings: [],
    issues: [],
    ...extra,
  };
}

/**
 * Revision 5 on a fleet of three: the attic AP's Wi-Fi and the gateway's network and DHCP change
 * (dnsmasq on, a renamed section, a masked key), the hall AP is up to date, the shed in observe mode.
 */
export function applyPlan(): ApplyPlan {
  return {
    revision: 5,
    nodes: [
      nodePlan(2, "Attic AP", {
        action: "change",
        hops: 2,
        packages: [
          {
            package: "wireless",
            sections: [
              {
                before: { type: "wifi-iface", name: "default_radio0", index: 0 },
                after: { type: "wifi-iface", name: "uw_home_2g", index: 0 },
                moved: false,
                options: [
                  { name: "ssid", before: "OpenWrt", after: "Home" },
                  { name: "key", before: "••••••••", after: "••••••••" },
                  { name: "ieee80211k", before: null, after: "1" },
                ],
              },
            ],
          },
        ],
        services: [{ name: "dnsmasq", enable: false }],
      }),
      nodePlan(1, "Gateway", {
        action: "change",
        role: "gateway",
        hops: 0,
        timeout_s: 300,
        raw: [
          {
            package: "network",
            type: "interface",
            name: "wg0",
            options: {
              proto: "wireguard",
              private_key: { secret: "config/77777777777777777777777777777777" },
              addresses: ["10.9.0.1/24"],
            },
            merge: false,
          },
          {
            package: "firewall",
            type: "zone",
            name: null,
            options: { name: "lan", log: "1" },
            merge: true,
          },
        ],
        packages: [
          {
            package: "network",
            sections: [
              {
                before: null,
                after: { type: "device", index: 1 },
                moved: false,
                options: [
                  { name: "name", before: null, after: "br-lan.20" },
                  { name: "ports", before: null, after: ["lan1:t", "lan2:t"] },
                ],
              },
            ],
          },
          { package: "dhcp", sections: [] },
        ],
        address: { from: "192.168.1.1", to: "10.0.0.1" },
        warnings: [{ path: "networks/lan", message: "192.168.1.0/24 is OpenWrt's factory subnet" }],
      }),
      nodePlan(3, "Hall AP"),
      nodePlan(4, "Shed AP", { action: "skipped", reason: "Observe mode", hops: null }),
    ],
  };
}

/** An apply of revision 5 with each node's state. */
export function applyJob(state: Job["state"], nodes: Partial<JobNode>[] = []): Job {
  return {
    id: 21,
    kind: "apply",
    target: "revision 5",
    node: null,
    revision: 5,
    state,
    steps: (["connect", "plan", "stage", "apply", "verify", "confirm", "record"] as const).map(
      (name) => ({
        name,
        state: state === "succeeded" ? "done" : "pending",
        detail: null,
        started_at: null,
        finished_at: null,
      }),
    ),
    nodes: nodes.map((node, i) => ({
      node: i + 1,
      name: `Node ${i + 1}`,
      state: "pending",
      detail: null,
      ...node,
    })),
    error: null,
    created_by: 1,
    started_at: 0,
    finished_at: null,
  };
}
