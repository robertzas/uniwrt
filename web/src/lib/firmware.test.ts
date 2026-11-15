import { describe, expect, it } from "vitest";
import type { FirmwareOffer } from "../api/gen/FirmwareOffer";
import type { NodeFirmware } from "../api/gen/NodeFirmware";
import type { NodeSummary } from "../api/gen/NodeSummary";
import { defaultOffer, firmwareRows, firmwareState, tightTmp } from "./firmware";

const offer = (release: string, kind: "update" | "branch", profile: string | null = "p") =>
  ({ release, kind, notes: "", profile, image_bytes: 10e6 }) satisfies FirmwareOffer;

const node = (id: number, version = "24.10.7", offers: FirmwareOffer[] = []): NodeFirmware => ({
  node: id,
  distribution: "OpenWrt",
  version,
  revision: "r1",
  target: "mediatek/filogic",
  board: "glinet,gl-mt3000",
  offers,
  note: null,
});

describe("firmwareState", () => {
  it("tells up to date, available and unknown apart", () => {
    expect(firmwareState(node(1, "25.12.6"), false)).toBe("current");
    expect(firmwareState(node(1, "24.10.7", [offer("24.10.8", "update")]), false)).toBe(
      "available",
    );
    expect(firmwareState(node(1, "24.10.7", [offer("25.12.6", "branch", null)]), false)).toBe(
      "current",
    );
    expect(firmwareState({ ...node(1), note: "a snapshot" }, false)).toBe("unknown");
    expect(firmwareState(node(1, ""), false)).toBe("unknown");
    expect(firmwareState(node(1, "25.12.6"), true)).toBe("unknown");
  });
});

describe("firmwareRows", () => {
  it("puts the gateway first, then names, and keeps buildable offers", () => {
    const nodes = [
      { id: 1, name: "Office", role: "ap" },
      { id: 2, name: "Gateway", role: "gateway" },
      { id: 3, name: "Attic", role: "ap" },
    ] as NodeSummary[];
    const rows = firmwareRows(
      {
        nodes: [
          node(1, "24.10.7", [offer("25.12.6", "branch", null), offer("24.10.8", "update")]),
          node(2),
          node(3),
        ],
        error: null,
      },
      nodes,
    );
    expect(rows.map((row) => row.node?.name)).toEqual(["Gateway", "Attic", "Office"]);
    expect(rows[2]?.buildable.map((o) => o.release)).toEqual(["24.10.8"]);
  });
});

describe("defaultOffer", () => {
  it("prefers the update within the branch", () => {
    expect(defaultOffer([offer("25.12.6", "branch"), offer("24.10.8", "update")])?.release).toBe(
      "24.10.8",
    );
    expect(defaultOffer([offer("25.12.6", "branch")])?.release).toBe("25.12.6");
    expect(defaultOffer([])).toBeUndefined();
  });
});

describe("tightTmp", () => {
  it("wants a third more than the image and a margin free", () => {
    const mb = 1024 * 1024;
    expect(tightTmp({ image_bytes: 10 * mb, tmp_free_bytes: 200 * mb })).toBe(false);
    expect(tightTmp({ image_bytes: 10 * mb, tmp_free_bytes: 25 * mb })).toBe(true);
    expect(tightTmp({ image_bytes: null, tmp_free_bytes: 1 })).toBe(false);
  });
});
