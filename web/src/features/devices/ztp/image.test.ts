import { describe, expect, it } from "vitest";
import type { FirmwareProfile } from "../../../api/gen/FirmwareProfile";
import { matchProfiles, packageChanges } from "./image";

const profile = (id: string, ...titles: string[]): FirmwareProfile => ({
  id,
  target: "mediatek/filogic",
  titles,
});

const catalog = [
  profile("asus_rt-ax59u", "ASUS RT-AX59U"),
  profile("glinet_gl-mt3000", "GL.iNet GL-MT3000"),
  profile("glinet_gl-mt6000", "GL.iNet GL-MT6000"),
  profile("xiaomi_ax3000t", "Xiaomi AX3000T"),
  profile("tplink_archer-c7-v5", "TP-Link Archer C7 v5", "TP-Link Archer C7 v5.1"),
];

describe("matchProfiles", () => {
  it("needs every word, in a title or the ID", () => {
    expect(matchProfiles(catalog, "gl mt6000").shown.map((p) => p.id)).toEqual([
      "glinet_gl-mt6000",
    ]);
    expect(matchProfiles(catalog, "archer-c7").shown.map((p) => p.id)).toEqual([
      "tplink_archer-c7-v5",
    ]);
    expect(matchProfiles(catalog, "  ").shown).toEqual([]);
    expect(matchProfiles(catalog, "nosuch").shown).toEqual([]);
  });

  it("puts titles that start with the query first, then words that do", () => {
    expect(matchProfiles(catalog, "ax3000").shown.map((p) => p.id)).toEqual(["xiaomi_ax3000t"]);
    expect(matchProfiles(catalog, "3000").shown.map((p) => p.id)).toEqual([
      "glinet_gl-mt3000",
      "xiaomi_ax3000t",
    ]);
    expect(matchProfiles(catalog, "a").shown.map((p) => p.id)).toEqual([
      "asus_rt-ax59u",
      "xiaomi_ax3000t",
      "tplink_archer-c7-v5",
    ]);
  });

  it("counts the matches it doesn't show", () => {
    expect(matchProfiles(catalog, "gl", 1)).toEqual({ shown: [catalog[1]], more: 1 });
  });
});

describe("packageChanges", () => {
  it("splits added and replaced packages", () => {
    expect(
      packageChanges(["-wpad-basic-mbedtls", "wpad-mbedtls", "kmod-batman-adv", "batctl-default"]),
    ).toEqual({
      added: ["wpad-mbedtls", "kmod-batman-adv", "batctl-default"],
      removed: ["wpad-basic-mbedtls"],
    });
  });
});
