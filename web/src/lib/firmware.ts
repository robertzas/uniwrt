import { type Accessor, createEffect, createSignal, onSettled } from "solid-js";
import type { FirmwareCatalog } from "../api/gen/FirmwareCatalog";
import type { FirmwareOffer } from "../api/gen/FirmwareOffer";
import type { FirmwareUpload } from "../api/gen/FirmwareUpload";
import type { GitHubRelease } from "../api/gen/GitHubRelease";
import type { ImageBuild } from "../api/gen/ImageBuild";
import type { NodeFirmware } from "../api/gen/NodeFirmware";
import type { NodeSource } from "../api/gen/NodeSource";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { PlanBody } from "../api/gen/PlanBody";
import type { UpgradeInstallBody } from "../api/gen/UpgradeInstallBody";
import type { UpgradePlan } from "../api/gen/UpgradePlan";
import { api, errorText } from "./api";

/**
 * Where a node's firmware stands; `listed` when a GitHub repo's releases are on offer (whether it
 * runs one of them can't be told).
 */
export type FirmwareState = "current" | "available" | "listed" | "unknown";

/** A node of the catalog with what the fleet knows of it. */
export interface FirmwareRow {
  firmware: NodeFirmware;
  node: NodeSummary | undefined;
  state: FirmwareState;
  /** The offers it can build an image of (the release has one for its board). */
  buildable: FirmwareOffer[];
  /** The uploaded images made for its board, newest first. */
  uploads: FirmwareUpload[];
  /** Its GitHub repo's releases with an image for it, newest first. */
  releases: GitHubRelease[];
}

/** Whether an admin can prepare an upgrade of the node: a release or an upload on offer. */
export function hasImages(firmware: NodeFirmware): boolean {
  return (
    firmware.offers.some((offer) => offer.profile !== null) ||
    firmware.uploads.length > 0 ||
    firmware.releases.length > 0
  );
}

/** Whether an admin can prepare an upgrade of the row. */
export function preparable(row: FirmwareRow): boolean {
  return row.buildable.length > 0 || row.uploads.length > 0 || row.releases.length > 0;
}

export function firmwareState(firmware: NodeFirmware, catalogError: boolean): FirmwareState {
  if (firmware.version === "") return "unknown";
  // A GitHub source doesn't depend on ASU, whose error is the catalog's.
  if (firmware.repo !== null) return firmware.releases.length > 0 ? "listed" : "unknown";
  if (catalogError) return "unknown";
  if (firmware.offers.some((offer) => offer.profile !== null)) return "available";
  return firmware.note === null ? "current" : "unknown";
}

/** The catalog's nodes, the gateway first, then by name. */
export function firmwareRows(
  catalog: FirmwareCatalog,
  nodes: readonly NodeSummary[],
): FirmwareRow[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const uploads = new Map(catalog.uploads.map((upload) => [upload.sha256, upload]));
  const rows = catalog.nodes.map((firmware) => ({
    firmware,
    node: byId.get(firmware.node),
    state: firmwareState(firmware, catalog.error !== null),
    buildable: firmware.offers.filter((offer) => offer.profile !== null),
    uploads: firmware.uploads.flatMap((sha) => uploads.get(sha) ?? []),
    releases: firmware.releases,
  }));
  const rank = (row: FirmwareRow) => (row.node?.role === "gateway" ? 0 : 1);
  return rows.sort(
    (a, b) =>
      rank(a) - rank(b) ||
      (a.node?.name ?? "").localeCompare(b.node?.name ?? "") ||
      a.firmware.node - b.firmware.node,
  );
}

/**
 * An image to upgrade with: an OpenWrt release (built by ASU or OpenWrt's own image), an upload,
 * or a GitHub release of the node's repo.
 */
export type ImageChoice =
  | { kind: "release"; release: string }
  | { kind: "upload"; sha256: string }
  | { kind: "github"; tag: string };

/** The choice's key in a radio group. */
export function choiceKey(choice: ImageChoice): string {
  switch (choice.kind) {
    case "release":
      return `release:${choice.release}`;
    case "upload":
      return `upload:${choice.sha256}`;
    case "github":
      return `github:${choice.tag}`;
  }
}

/** The plan request of a choice; an OpenWrt release's with its source. */
export function planBody(choice: ImageChoice, official: boolean): PlanBody {
  if (choice.kind === "upload") return { source: "upload", sha256: choice.sha256 };
  if (choice.kind === "github") return { source: "github", release: choice.tag };
  return { source: official ? "official" : "asu", release: choice.release };
}

/**
 * What installs a choice (D-124): an OpenWrt release built by ASU needs its finished `build` (its
 * hash); `undefined` until it is done.
 */
export function installBody(
  choice: ImageChoice,
  official: boolean,
  build: ImageBuild | undefined,
): UpgradeInstallBody | undefined {
  if (choice.kind !== "release") return planBody(choice, official) as UpgradeInstallBody;
  if (official) return { source: "official", release: choice.release };
  return build?.state === "done"
    ? { source: "asu", release: choice.release, build: build.hash }
    : undefined;
}

/** How an OpenWrt release is planned first: as the node's source says (ASU unless official). */
export function openwrtSource(source: NodeSource): "asu" | "official" {
  return source.kind === "official" ? "official" : "asu";
}

/** The offer to start from: the update within the branch (fixes only), else the next branch. */
export function defaultOffer(offers: readonly FirmwareOffer[]): FirmwareOffer | undefined {
  return offers.find((offer) => offer.kind === "update") ?? offers[0];
}

/** Room an upgrade needs in `/tmp` beyond the image: sysupgrade's own and a margin. */
const STAGING_MARGIN = 16 * 1024 * 1024;

/**
 * Whether `/tmp` is short for staging the image: ASU's image with the node's packages is somewhat
 * larger than the official one, so a third more than its size plus a margin must be free; an
 * official or uploaded image's size is exact.
 */
export function tightTmp(
  plan: Pick<UpgradePlan, "image_bytes" | "tmp_free_bytes" | "source">,
): boolean {
  if (plan.image_bytes === null || plan.tmp_free_bytes === null) return false;
  const growth = plan.source === "asu" ? 1.33 : 1;
  return plan.tmp_free_bytes < plan.image_bytes * growth + STAGING_MARGIN;
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
