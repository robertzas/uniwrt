import type { Job } from "../api/gen/Job";
import type { NodeDetail } from "../api/gen/NodeDetail";
import type { NodeFacts } from "../api/gen/NodeFacts";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { StepState } from "../api/gen/StepState";

/* Made-up API data for tests. */

export function summary(id: number, name: string, extra: Partial<NodeSummary> = {}): NodeSummary {
  return {
    id,
    name,
    role: "ap",
    mode: "observe",
    host: `192.168.1.${id}`,
    mac: `02:00:00:00:00:0${id}`,
    status: { state: "online", since: 0, reasons: [] },
    model: "Test router",
    version: "24.10.4",
    uptime_s: 100,
    cpu_percent: 5,
    memory_percent: 30,
    temperature_c: null,
    clients: 2,
    collected_at: 0,
    ...extra,
  };
}

export function facts(cpu: number): NodeFacts {
  return {
    collected_at: 1,
    board: {
      hostname: "gw",
      model: "Test router",
      board_name: "test,router",
      cpu: "ARMv8",
      kernel: "6.6",
      rootfs: "squashfs",
      label_mac: null,
      release: {
        distribution: "OpenWrt",
        version: "24.10.4",
        revision: "r1",
        target: "mediatek/filogic",
        arch: "aarch64",
      },
    },
    system: {
      uptime_s: 100,
      node_time: null,
      load: [0.1, 0.2, 0.3],
      cpu_percent: cpu,
      memory: {
        total_bytes: 1000,
        available_bytes: 750,
        free_bytes: 500,
        cached_bytes: 0,
        buffered_bytes: 0,
      },
      root: null,
      tmp: null,
      temperature_c: 50,
    },
    capabilities: {} as NodeFacts["capabilities"],
    interfaces: [],
    devices: [],
    phys: [],
    radios: [],
    stations: [],
    leases: [],
    neighbors: [],
    fdb: [],
    batman: [],
  };
}

export function detail(withFacts: NodeFacts | null): NodeDetail {
  const s = summary(1, "gw", { role: "gateway" });
  return {
    node: {
      id: 1,
      name: s.name,
      mac: s.mac,
      board_name: "test,router",
      host: s.host,
      port: 22,
      role: "gateway",
      mode: "observe",
      created_at: 0,
      updated_at: 0,
    },
    status: s.status,
    facts: withFacts,
  };
}

const adoptSteps = [
  "connect",
  "check",
  "install_key",
  "verify_key",
  "inventory",
  "backup",
  "record",
  "watch",
] as const;

/** An adoption of `host` whose first `done` steps are done and the next one is in `state`. */
export function adoptJob(
  id: number,
  host: string,
  done: number,
  state: StepState = "running",
  extra: Partial<Job> = {},
): Job {
  return {
    id,
    kind: "adopt",
    target: `root@${host}:22`,
    node: null,
    state: "running",
    steps: adoptSteps.map((name, i) => ({
      name,
      state: i < done ? "done" : i === done ? state : "pending",
      detail: i < done ? `${name} detail` : null,
      started_at: i <= done ? 1 : null,
      finished_at: i < done ? 2 : null,
    })),
    error: null,
    created_by: 1,
    started_at: 1,
    finished_at: null,
    ...extra,
  };
}
