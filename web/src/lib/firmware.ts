import { type Accessor, createEffect, createSignal, onSettled } from "solid-js";
import type { FirmwareCatalog } from "../api/gen/FirmwareCatalog";
import type { FirmwareOffer } from "../api/gen/FirmwareOffer";
import type { ImageBuild } from "../api/gen/ImageBuild";
import type { NodeFirmware } from "../api/gen/NodeFirmware";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { UpgradePlan } from "../api/gen/UpgradePlan";
import { api, errorText } from "./api";

/** Where a node's firmware stands. */
export type FirmwareState = "current" | "available" | "unknown";

/** A node of the catalog with what the fleet knows of it. */
export interface FirmwareRow {
  firmware: NodeFirmware;
  node: NodeSummary | undefined;
  state: FirmwareState;
  /** The offers it can build an image of (the release has one for its board). */
  buildable: FirmwareOffer[];
}

export function firmwareState(firmware: NodeFirmware, catalogError: boolean): FirmwareState {
  if (catalogError || firmware.version === "") return "unknown";
  if (firmware.offers.some((offer) => offer.profile !== null)) return "available";
  return firmware.note === null ? "current" : "unknown";
}

/** The catalog's nodes, the gateway first, then by name. */
export function firmwareRows(
  catalog: FirmwareCatalog,
  nodes: readonly NodeSummary[],
): FirmwareRow[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const rows = catalog.nodes.map((firmware) => ({
    firmware,
    node: byId.get(firmware.node),
    state: firmwareState(firmware, catalog.error !== null),
    buildable: firmware.offers.filter((offer) => offer.profile !== null),
  }));
  const rank = (row: FirmwareRow) => (row.node?.role === "gateway" ? 0 : 1);
  return rows.sort(
    (a, b) =>
      rank(a) - rank(b) ||
      (a.node?.name ?? "").localeCompare(b.node?.name ?? "") ||
      a.firmware.node - b.firmware.node,
  );
}

/** The offer to start from: the update within the branch (fixes only), else the next branch. */
export function defaultOffer(offers: readonly FirmwareOffer[]): FirmwareOffer | undefined {
  return offers.find((offer) => offer.kind === "update") ?? offers[0];
}

/** Room an upgrade needs in `/tmp` beyond the image: sysupgrade's own and a margin. */
const STAGING_MARGIN = 16 * 1024 * 1024;

/**
 * Whether `/tmp` is short for staging the image: an image with the node's packages is somewhat
 * larger than the official one, so a third more than its size plus a margin must be free.
 */
export function tightTmp(plan: Pick<UpgradePlan, "image_bytes" | "tmp_free_bytes">): boolean {
  if (plan.image_bytes === null || plan.tmp_free_bytes === null) return false;
  return plan.tmp_free_bytes < plan.image_bytes * 1.33 + STAGING_MARGIN;
}

/** Fetches the firmware catalog (`GET v1/firmware`) once mounted, and again on `reload`. */
export function createCatalog() {
  const [catalog, setCatalog] = createSignal<FirmwareCatalog>();
  const [error, setError] = createSignal<string>();
  const [loading, setLoading] = createSignal(false);
  let controller: AbortController | undefined;
  const reload = () => {
    controller?.abort();
    const current = new AbortController();
    controller = current;
    setLoading(true);
    setError(undefined);
    api.firmware
      .catalog(current.signal)
      .then((found) => {
        setCatalog(found);
      })
      .catch((err) => {
        if (!current.signal.aborted) setError(errorText(err));
      })
      .finally(() => {
        if (controller === current) setLoading(false);
      });
  };
  onSettled(() => {
    reload();
    return () => controller?.abort();
  });
  return { catalog, error, loading, reload };
}

/** How often a build at ASU is read again while it is queued or building. */
export const FOLLOW_MS = 3000;

/**
 * Reads `build` again from ASU every few seconds while it is queued or building and `active`
 * holds (its dialog open), through `fetch`; errors go to `onError`, cleared by the next read.
 */
export function followBuild(options: {
  build: Accessor<ImageBuild | undefined>;
  active: Accessor<boolean>;
  fetch: (hash: string, signal: AbortSignal) => Promise<ImageBuild>;
  onBuild: (build: ImageBuild) => void;
  onError: (text: string | undefined) => void;
}) {
  createEffect(
    () => {
      const current = options.build();
      const running = current?.state === "queued" || current?.state === "building";
      return options.active() && running ? current?.hash : undefined;
    },
    (hash) => {
      if (!hash) return;
      const controller = new AbortController();
      const timer = setInterval(async () => {
        try {
          options.onBuild(await options.fetch(hash, controller.signal));
          options.onError(undefined);
        } catch (err) {
          if (!controller.signal.aborted) options.onError(errorText(err));
        }
      }, FOLLOW_MS);
      return () => {
        controller.abort();
        clearInterval(timer);
      };
    },
  );
}
