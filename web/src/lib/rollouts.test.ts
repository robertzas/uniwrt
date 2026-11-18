import { describe, expect, it } from "vitest";
import type { FirmwareOffer } from "../api/gen/FirmwareOffer";
import type { NodeFirmware } from "../api/gen/NodeFirmware";
import type { FirmwareRow } from "./firmware";
import {
  choiceLabel,
  choiceShort,
  defaultChoice,
  isActive,
  moving,
  newestOffer,
  previewOrder,
  rowChoices,
} from "./rollouts";

const offer = (release: string, kind: "update" | "branch"): FirmwareOffer => ({
  release,
  kind,
  notes: "",
  profile: "p",
  image_bytes: 1,
});

const row = (
  extra: Partial<FirmwareRow> = {},
  firmware: Partial<NodeFirmware> = {},
): FirmwareRow => ({
  firmware: {
    node: 2,
    distribution: "OpenWrt",
    version: "24.10.7",
    revision: "r1",
    target: "mediatek/filogic",
    board: "b",
    offers: [],
    note: null,
    uploads: [],
    source: { kind: "site" },
    repo: null,
    releases: [],
    ...firmware,
  },
  node: undefined,
  state: "available",
  buildable: [offer("24.10.8", "update"), offer("25.12.6", "branch")],
  uploads: [],
  releases: [],
  ...extra,
});

const upload = {
  sha256: "ab".repeat(32),
  name: "custom.bin",
  size_bytes: 1,
  uploaded_at: 0,
  target: "t",
  boards: [],
  distribution: null,
  version: null,
  revision: null,
};

const release = (tag: string) => ({
  tag,
  name: tag,
  prerelease: false,
  published_at: null,
  notes: "",
  images: [],
});

describe("rollout choices", () => {
  it("starts each device on its newest offer from its own source", () => {
    expect(newestOffer([offer("24.10.8", "update"), offer("25.12.10", "branch")])?.release).toBe(
      "25.12.10",
    );
    expect(defaultChoice(row())).toEqual({ source: "asu", release: "25.12.6" });
    expect(defaultChoice(row({}, { source: { kind: "official" } }))).toEqual({
      source: "official",
      release: "25.12.6",
    });
    expect(defaultChoice(row({ buildable: [], releases: [release("v3"), release("v2")] }))).toEqual(
      { source: "github", release: "v3" },
    );
    expect(defaultChoice(row({ buildable: [], uploads: [upload] }))).toEqual({
      source: "upload",
      sha256: upload.sha256,
    });
    expect(defaultChoice(row({ buildable: [] }))).toBeUndefined();
  });

  it("offers every image, newest release first, both ways of building it", () => {
    const all = rowChoices(row({ uploads: [upload] }));
    expect(all).toEqual([
      { source: "asu", release: "25.12.6" },
      { source: "official", release: "25.12.6" },
      { source: "asu", release: "24.10.8" },
      { source: "official", release: "24.10.8" },
      { source: "upload", sha256: upload.sha256 },
    ]);
    const labelled = row({ uploads: [upload] });
    expect(all.map((choice) => choiceLabel(choice, labelled))).toEqual([
      "OpenWrt 25.12.6 (new release) · built by ASU",
      "OpenWrt 25.12.6 (new release) · official image",
      "OpenWrt 24.10.8 · built by ASU",
      "OpenWrt 24.10.8 · official image",
      "Uploaded · custom.bin",
    ]);
    expect(choiceShort({ source: "asu", release: "25.12.6" }, [])).toBe("OpenWrt 25.12.6");
    expect(choiceShort({ source: "upload", sha256: upload.sha256 }, [upload])).toBe("custom.bin");
    expect(choiceShort({ source: "github", release: "v3" }, [])).toBe("v3");
  });
});

describe("previewOrder", () => {
  const entries = [
    { node: 1, name: "Gateway", gateway: true },
    { node: 3, name: "Office", gateway: false },
    { node: 2, name: "Lab", gateway: false },
    { node: 4, name: "Attic", gateway: false },
  ];
  const names = (canary: number | null | undefined) =>
    previewOrder(entries, canary).map((entry) => entry.name);

  it("puts the gateway last and the chosen canary first", () => {
    expect(names(undefined)).toEqual(["Attic", "Lab", "Office", "Gateway"]);
    expect(names(null)).toEqual(["Attic", "Lab", "Office", "Gateway"]);
    expect(names(3)).toEqual(["Office", "Attic", "Lab", "Gateway"]);
    // The gateway is never the canary of several.
    expect(names(1)).toEqual(["Attic", "Lab", "Office", "Gateway"]);
  });
});

describe("rollout states", () => {
  it("tells active from moving", () => {
    expect(
      ["running", "paused", "halted", "done", "cancelled"].map((s) => isActive(s as never)),
    ).toEqual([true, true, true, false, false]);
    expect(["running", "paused", "halted"].map((s) => moving(s as never))).toEqual([
      true,
      true,
      false,
    ]);
  });
});
