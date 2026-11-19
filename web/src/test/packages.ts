import type { InstalledPackage } from "../api/gen/InstalledPackage";
import type { NodePackages } from "../api/gen/NodePackages";
import type { PackageCatalog } from "../api/gen/PackageCatalog";

export function installed(name: string, extra: Partial<InstalledPackage> = {}): InstalledPackage {
  return { name, version: "1.0-r1", explicit: true, lock: null, with_firmware: false, ...extra };
}

/** A gateway's packages: locked ones, one from the firmware, a dependency and one removable. */
export function nodePackages(extra: Partial<NodePackages> = {}): NodePackages {
  return {
    taken_at: Date.UTC(2026, 10, 19, 9, 0),
    packages: [
      installed("curl", { version: "8.10.1-r1" }),
      installed("dropbear", { lock: "essential" }),
      installed("htop", { version: "3.3.0-r1" }),
      installed("kernel", { version: "6.12.40-1", lock: "essential", with_firmware: true }),
      installed("libncurses6", { explicit: false }),
      installed("sqm-scripts", { lock: "required" }),
    ],
    required_unknown: null,
    ...extra,
  };
}

/** No inventory yet, for the tests of other cards on the Config tab. */
export const NO_PACKAGES: NodePackages = { taken_at: null, packages: [], required_unknown: null };

export function packageCatalog(): PackageCatalog {
  return {
    read_at: Date.UTC(2026, 10, 19, 10, 0),
    available: [
      { name: "curl", version: "8.11.0-r1", installed_bytes: 300_000, description: "URL tool" },
      {
        name: "iperf3",
        version: "3.17-r1",
        installed_bytes: 120_000,
        description: "Network bandwidth measurement",
      },
      {
        name: "tcpdump",
        version: "4.99.5-r1",
        installed_bytes: 900_000,
        description: "Packet capture",
      },
      {
        name: "tcpdump-mini",
        version: "4.99.5-r1",
        installed_bytes: null,
        description: "Packet capture, small",
      },
      {
        name: "nano",
        version: "8.2-r1",
        installed_bytes: 200_000,
        description: "A small text editor",
      },
    ],
    upgrades: [
      { name: "curl", from: "8.10.1-r1", to: "8.11.0-r1" },
      { name: "kernel", from: "6.12.40-1", to: "6.12.41-1" },
    ],
  };
}
