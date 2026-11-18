import { describe, expect, it } from "vitest";
import type { Issue } from "../../../api/gen/Issue";
import type { SecretImport } from "../../../api/gen/SecretImport";
import { filledSecrets, readSiteFile, sortFileIssues } from "./file";

const secrets: SecretImport[] = [
  { path: "wlans/home/passphrase", source: "file" },
  { path: "wlans/guest/passphrase", source: "missing" },
  { path: "wan/pppoe/password", source: "missing" },
];

describe("readSiteFile", () => {
  it("reads what the page shows of a site file", () => {
    const text = JSON.stringify({
      format: "uniwrt-site",
      revision: 4,
      uniwrt: "1.0.0",
      exported_at: 1000,
      secrets: "-----BEGIN AGE ENCRYPTED FILE-----",
    });
    expect(readSiteFile("a.json", text)).toMatchObject({
      name: "a.json",
      revision: 4,
      uniwrt: "1.0.0",
      exportedAt: 1000,
      secrets: true,
    });
    expect(readSiteFile("a.json", '{"format": "uniwrt-site", "secrets": null}')).toMatchObject({
      secrets: false,
    });
  });

  it("refuses what isn't JSON or a site file", () => {
    expect(readSiteFile("a", "{")).toHaveProperty("error");
    expect(readSiteFile("a", "[]")).toHaveProperty("error");
    expect(readSiteFile("a", '{"format": "uniwrt-backup"}')).toHaveProperty("error");
  });
});

describe("the missing secrets", () => {
  it("sends only the missing ones given a value", () => {
    const values = {
      "wlans/home/passphrase": "not sent",
      "wlans/guest/passphrase": "guest key",
      "wan/pppoe/password": "",
    };
    expect(filledSecrets(secrets, values)).toEqual({ "wlans/guest/passphrase": "guest key" });
  });

  it("lets a value clear its secret's error, and nothing else", () => {
    const issues: Issue[] = [
      { path: "wlans/guest/passphrase", severity: "error", message: "needs a passphrase" },
      { path: "wan/pppoe/password", severity: "error", message: "needs a password" },
      { path: "networks/lan", severity: "error", message: "overlaps" },
      { path: "site/ntp_servers", severity: "warning", message: "none" },
    ];
    const sorted = sortFileIssues(issues, secrets, { "wlans/guest/passphrase": "guest key" });
    expect(sorted.errors).toHaveLength(3);
    expect(sorted.warnings.map((i) => i.path)).toEqual(["site/ntp_servers"]);
    expect(sorted.blocking.map((i) => i.path)).toEqual(["wan/pppoe/password", "networks/lan"]);
    expect([...sorted.bySecret.keys()]).toEqual(["wlans/guest/passphrase", "wan/pppoe/password"]);
  });
});
