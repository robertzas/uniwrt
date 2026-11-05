import type { ImportPreview } from "../api/gen/ImportPreview";
import type { NodeImportView } from "../api/gen/NodeImportView";

/** A node's part of an import: by default an imported access point in observe mode. */
export function nodeImport(
  node: number,
  name: string,
  extra: Partial<NodeImportView> = {},
): NodeImportView {
  return {
    node,
    name,
    role: "ap",
    mode: "observe",
    state: "imported",
    reason: null,
    confidence: 92,
    sections: [],
    residual: [],
    issues: [],
    warnings: [],
    ...extra,
  };
}

/**
 * Importing a fleet of three into revision 1: the gateway imported (a renamed interface, one
 * kept raw, a zone with an option kept), the attic AP imported with its firewall dropped, the
 * shed AP unread; the site's LAN and Wi-Fi change, the Wi-Fi key sealed.
 */
export function importPreview(): ImportPreview {
  return {
    id: "p-1",
    base: 1,
    changes: [
      {
        path: ["networks", "lan", "ipv4", "address"],
        op: "changed",
        before: "192.168.1.1/24",
        after: "10.0.0.1/24",
      },
      {
        path: ["wlans", "home"],
        op: "added",
        before: null,
        after: {
          id: "home",
          ssid: "Home",
          passphrase: { secret: "config/33333333333333333333333333333333" },
        },
      },
    ],
    issues: [
      { path: "nodes/2/raw/0", severity: "warning", message: "Ignored while wireless is off" },
      { path: "nodes/2/raw/1", severity: "warning", message: "Ignored while wireless is off" },
    ],
    nodes: [
      nodeImport(1, "Gateway", {
        role: "gateway",
        confidence: 93,
        sections: [
          {
            package: "network",
            type: "interface",
            name: "lan",
            renamed: null,
            fate: "mapped",
            kept: [],
          },
          {
            package: "network",
            type: "interface",
            name: "batwired",
            renamed: null,
            fate: "raw",
            kept: [],
          },
          {
            package: "firewall",
            type: "zone",
            name: null,
            renamed: "lan",
            fate: "mapped",
            kept: ["log", "mtu_fix"],
          },
        ],
        residual: [
          {
            package: "dhcp",
            sections: [
              {
                before: { type: "dnsmasq", index: 0 },
                after: { type: "dnsmasq", index: 0 },
                moved: false,
                options: [{ name: "force", before: null, after: "1" }],
              },
            ],
          },
        ],
      }),
      nodeImport(2, "Attic AP", {
        sections: [
          {
            package: "firewall",
            type: "zone",
            name: null,
            renamed: null,
            fate: "dropped",
            kept: [],
          },
        ],
      }),
      nodeImport(4, "Shed AP", {
        state: "unread",
        confidence: 0,
        reason: "The node did not answer: connection refused",
      }),
    ],
  };
}
