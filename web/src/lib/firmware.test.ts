import { describe, expect, it } from "vitest";
import type { FirmwareOffer } from "../api/gen/FirmwareOffer";
import type { FirmwareUpload } from "../api/gen/FirmwareUpload";
import type { GitHubRelease } from "../api/gen/GitHubRelease";
import type { ImageBuild } from "../api/gen/ImageBuild";
import type { NodeFirmware } from "../api/gen/NodeFirmware";
import type { NodeSummary } from "../api/gen/NodeSummary";
import {
  choiceKey,
  defaultOffer,
  firmwareRows,
  firmwareState,
  installBody,
  openwrtSource,
  planBody,
  preparable,
  releaseSupported,
  tightTmp,
} from "./firmware";
import { strings } from "./strings";

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
  uploads: [],
  source: { kind: "site" },
  repo: null,
  releases: [],
});

const release = (tag: string): GitHubRelease => ({
  tag,
  name: tag,
  prerelease: false,
  published_at: null,
  notes: "",
  images: [],
});

const upload = (sha256: string): FirmwareUpload => ({
  sha256,
  name: `${sha256}.bin`,
  size_bytes: 1,
  uploaded_at: 0,
  target: "mediatek/filogic",
  boards: ["glinet,gl-mt3000"],
  distribution: null,
  version: null,
  revision: null,
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

  it("lists a GitHub repo's releases whatever ASU says", () => {
    const github = { ...node(1), repo: "owner/images", releases: [release("v2")] };
    expect(firmwareState(github, true)).toBe("listed");
    expect(firmwareState({ ...github, releases: [], note: "no image" }, false)).toBe("unknown");
  });
});

describe("the catalog's summary", () => {
  it("claims up to date only when every device is", () => {
    const summary = strings.firmware.summary;
    expect(summary(3, 0)).toBe("All 3 devices are up to date");
    expect(summary(3, 1, 0, 1)).toBe("1 of 3 devices can upgrade");
    expect(summary(3, 1, 1)).toBe(
      "1 of 3 devices can upgrade · 1 device with GitHub releases to choose from",
    );
    expect(summary(3, 0, 0, 3)).toBe("No upgrade known for 3 devices: see why below");
  });
});

describe("firmwareRows", () => {
  it("puts the gateway first, then names, and keeps buildable offers", () => {
    const nodes = [
      { id: 1, name: "Office", role: "ap" },
      { id: 2, name: "Gateway", role: "gateway" },
      { id: 3, name: "Attic", role: "ap" },
      { id: 4, name: "Shed", role: "ap" },
    ] as NodeSummary[];
    const rows = firmwareRows(
      {
        nodes: [
          node(1, "24.10.7", [offer("25.12.6", "branch", null), offer("24.10.8", "update")]),
          node(2),
          { ...node(3), uploads: ["b", "gone"] },
          { ...node(4), repo: "owner/images", releases: [release("v2")] },
        ],
        uploads: [upload("a"), upload("b")],
        error: null,
        sources: { repo: "owner/images", prereleases: false, token: false },
      },
      nodes,
    );
    expect(rows.map((row) => row.node?.name)).toEqual(["Gateway", "Attic", "Office", "Shed"]);
    expect(rows[2]?.buildable.map((o) => o.release)).toEqual(["24.10.8"]);
    // An upload the catalog doesn't list is left out.
    expect(rows[1]?.uploads.map((u) => u.sha256)).toEqual(["b"]);
    expect(rows.map(preparable)).toEqual([false, true, true, true]);
    expect(rows[3]?.state).toBe("listed");
  });
});

describe("installBody", () => {
  it("installs ASU's image once built, the others as planned", () => {
    const release = { kind: "release" as const, release: "25.12.6" };
    const build = (state: ImageBuild["state"]): ImageBuild => ({
      hash: "0123",
      state,
      queue_position: null,
      step: null,
      error: null,
      images: [],
      release: null,
    });
    expect(installBody(release, false, undefined)).toBeUndefined();
    expect(installBody(release, false, build("building"))).toBeUndefined();
    expect(installBody(release, false, build("done"))).toEqual({
      source: "asu",
      release: "25.12.6",
      build: "0123",
    });
    expect(installBody(release, true, undefined)).toEqual({
      source: "official",
      release: "25.12.6",
    });
    expect(installBody({ kind: "upload", sha256: "ab" }, false, undefined)).toEqual({
      source: "upload",
      sha256: "ab",
    });
    expect(installBody({ kind: "github", tag: "v2" }, false, undefined)).toEqual({
      source: "github",
      release: "v2",
    });
  });
});

describe("planBody", () => {
  it("names the source", () => {
    const release = { kind: "release", release: "25.12.6" } as const;
    expect(planBody(release, false)).toEqual({ source: "asu", release: "25.12.6" });
    expect(planBody(release, true)).toEqual({ source: "official", release: "25.12.6" });
    expect(planBody({ kind: "upload", sha256: "ab" }, true)).toEqual({
      source: "upload",
      sha256: "ab",
    });
    expect(planBody({ kind: "github", tag: "v2" }, true)).toEqual({
      source: "github",
      release: "v2",
    });
    expect(choiceKey({ kind: "github", tag: "v2" })).toBe("github:v2");
  });

  it("starts an OpenWrt release from the node's source", () => {
    expect(openwrtSource({ kind: "official" })).toBe("official");
    expect(openwrtSource({ kind: "site" })).toBe("asu");
    expect(openwrtSource({ kind: "github", repo: "a/b" })).toBe("asu");
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
    const asu = "asu" as const;
    expect(tightTmp({ image_bytes: 10 * mb, tmp_free_bytes: 200 * mb, source: asu })).toBe(false);
    expect(tightTmp({ image_bytes: 10 * mb, tmp_free_bytes: 25 * mb, source: asu })).toBe(true);
    expect(tightTmp({ image_bytes: null, tmp_free_bytes: 1, source: asu })).toBe(false);
    // An official image's size is exact.
    expect(tightTmp({ image_bytes: 10 * mb, tmp_free_bytes: 27 * mb, source: "official" })).toBe(
      false,
    );
  });
});

describe("releaseSupported", () => {
  it("takes OpenWrt from 24.10 and snapshots, as adoption's check does", () => {
    expect(releaseSupported("OpenWrt", "24.10.8")).toBe(true);
    expect(releaseSupported("OpenWrt", "25.12.0-rc1")).toBe(true);
    expect(releaseSupported("OpenWrt", "24.10-SNAPSHOT")).toBe(true);
    expect(releaseSupported("OpenWrt", "SNAPSHOT")).toBe(true);
    expect(releaseSupported("OpenWrt", "23.05.5")).toBe(false);
    expect(releaseSupported("OpenWrt", "")).toBe(false);
    expect(releaseSupported("ImmortalWrt", "24.10.0")).toBe(false);
  });
});
