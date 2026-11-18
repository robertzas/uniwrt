import { createEffect, createSignal, onSettled } from "solid-js";
import type { FirmwareOffer } from "../api/gen/FirmwareOffer";
import type { FirmwareUpload } from "../api/gen/FirmwareUpload";
import type { Rollout } from "../api/gen/Rollout";
import type { RolloutState } from "../api/gen/RolloutState";
import type { UpgradeChoice } from "../api/gen/UpgradeChoice";
import { api, errorText } from "./api";
import { type FirmwareRow, openwrtSource } from "./firmware";
import { strings } from "./strings";

const r = strings.firmware.rollouts.dialog;

/** Whether a rollout may still upgrade a node (one such at a time, D-125). */
export function isActive(state: RolloutState): boolean {
  return state === "running" || state === "paused" || state === "halted";
}

/** Whether the rollout changes by itself: a running one, or a paused one finishing its node. */
export function moving(state: RolloutState): boolean {
  return state === "running" || state === "paused";
}

/** OpenWrt releases newest first: `25.12.6` before `24.10.8`. */
export function byNewest(a: string, b: string): number {
  return b.localeCompare(a, "en", { numeric: true });
}

/** The newest offer, whatever its kind. */
export function newestOffer(offers: readonly FirmwareOffer[]): FirmwareOffer | undefined {
  return [...offers].sort((a, b) => byNewest(a.release, b.release))[0];
}

/**
 * Every image a row can roll out to, as the upgrade dialog offers them: its GitHub releases, each
 * OpenWrt release built by ASU or as the official image (newest first), its fitting uploads.
 */
export function rowChoices(row: FirmwareRow): UpgradeChoice[] {
  const offers = [...row.buildable].sort((a, b) => byNewest(a.release, b.release));
  return [
    ...row.releases.map((release) => ({ source: "github" as const, release: release.tag })),
    ...offers.flatMap((offer) => [
      { source: "asu" as const, release: offer.release },
      { source: "official" as const, release: offer.release },
    ]),
    ...row.uploads.map((upload) => ({ source: "upload" as const, sha256: upload.sha256 })),
  ];
}

/**
 * The image a row starts with (D-125): its newest offer from its own source (its repo's newest
 * release, OpenWrt's newest built as its source says), else its newest fitting upload.
 */
export function defaultChoice(row: FirmwareRow): UpgradeChoice | undefined {
  const release = row.releases[0];
  if (release) return { source: "github", release: release.tag };
  const offer = newestOffer(row.buildable);
  if (offer) return { source: openwrtSource(row.firmware.source), release: offer.release };
  const upload = row.uploads[0];
  return upload ? { source: "upload", sha256: upload.sha256 } : undefined;
}

/** A choice as a select's value. */
export function choiceValue(choice: UpgradeChoice): string {
  return choice.source === "upload"
    ? `upload:${choice.sha256}`
    : `${choice.source}:${choice.release}`;
}

/** A choice for people, with what the row knows of it. */
export function choiceLabel(
  choice: UpgradeChoice,
  row: Pick<FirmwareRow, "buildable" | "uploads"> & { firmware: { repo: string | null } },
): string {
  const kind = (release: string) =>
    r.kind(row.buildable.find((offer) => offer.release === release)?.kind ?? "");
  switch (choice.source) {
    case "asu":
      return r.asu(choice.release, kind(choice.release));
    case "official":
      return r.official(choice.release, kind(choice.release));
    case "github":
      return r.github(choice.release, row.firmware.repo ?? "GitHub");
    case "upload":
      return r.upload(
        row.uploads.find((upload) => upload.sha256 === choice.sha256)?.name ??
          choice.sha256.slice(0, 12),
      );
  }
}

/** A rollout node's image in short: `OpenWrt 25.12.6`, a tag, or an upload's name. */
export function choiceShort(choice: UpgradeChoice, uploads: readonly FirmwareUpload[]): string {
  if (choice.source === "upload") {
    return (
      uploads.find((upload) => upload.sha256 === choice.sha256)?.name ?? choice.sha256.slice(0, 12)
    );
  }
  return choice.source === "github" ? choice.release : `OpenWrt ${choice.release}`;
}

/** A device of a rollout being prepared. */
export interface OrderEntry {
  node: number;
  name: string;
  gateway: boolean;
}

/**
 * The order the controller is expected to run (D-125): the canary first (the chosen one, else the
 * first other device), the devices between it and the gateway (by name here: the controller orders
 * them by hops from the gateway, which only it knows), the gateway last. `canary` is `undefined`
 * without a canary, `null` to let the controller pick it.
 */
export function previewOrder(
  entries: readonly OrderEntry[],
  canary: number | null | undefined,
): OrderEntry[] {
  const leaves = entries
    .filter((entry) => !entry.gateway)
    .sort((a, b) => a.name.localeCompare(b.name) || a.node - b.node);
  const gateways = entries.filter((entry) => entry.gateway);
  if (canary !== undefined && entries.length > 1) {
    const first = leaves.findIndex((entry) => entry.node === canary);
    if (first > 0) leaves.unshift(...leaves.splice(first, 1));
  }
  return [...leaves, ...gateways];
}

/** The soak durations offered, in minutes (30 min to 24 h). */
export const SOAKS = [30, 60, 120, 240, 480, 720, 1440] as const;
/** The canary's default soak and the default window, 03:30–05:30 (D-125). */
export const DEFAULT_SOAK = 60;
export const DEFAULT_WINDOW = { start_min: 210, end_min: 330 } as const;

/** How often a moving rollout is read again. */
export const ROLLOUT_POLL_MS = 3000;
/** How many rollouts the list keeps, as the API answers. */
const LISTED = 20;

/**
 * The latest rollouts (`GET v1/firmware/rollouts`), read once mounted; the moving one (running,
 * or paused finishing its node) read again every few seconds (no WS topic, D-125). `onChange`
 * hears a rollout's state change seen by a read.
 */
export function createRollouts(options: { onChange?: (before: Rollout, after: Rollout) => void }) {
  const [rollouts, setRollouts] = createSignal<Rollout[]>();
  const [error, setError] = createSignal<string>();
  /** Takes in a rollout as the API answered it. */
  const put = (next: Rollout) => {
    const known = rollouts() ?? [];
    const before = known.find((rollout) => rollout.id === next.id);
    setRollouts(
      [next, ...known.filter((rollout) => rollout.id !== next.id)]
        .sort((a, b) => b.id - a.id)
        .slice(0, LISTED),
    );
    if (before && before.state !== next.state) options.onChange?.(before, next);
  };
  onSettled(() => {
    const controller = new AbortController();
    api.firmware.rollouts
      .list(controller.signal)
      .then((found) => {
        setRollouts(found);
      })
      .catch((err) => {
        if (!controller.signal.aborted) setError(errorText(err));
      });
    return () => controller.abort();
  });
  createEffect(
    () => rollouts()?.find((rollout) => moving(rollout.state))?.id,
    (id) => {
      if (id === undefined) return;
      const controller = new AbortController();
      const timer = setInterval(async () => {
        try {
          put(await api.firmware.rollouts.get(id, controller.signal));
          setError(undefined);
        } catch (err) {
          if (!controller.signal.aborted) setError(errorText(err));
        }
      }, ROLLOUT_POLL_MS);
      return () => {
        controller.abort();
        clearInterval(timer);
      };
    },
  );
  return {
    rollouts,
    error,
    put,
    /** The rollout that may still upgrade a node, if any. */
    active: () => rollouts()?.find((rollout) => isActive(rollout.state)),
  };
}
