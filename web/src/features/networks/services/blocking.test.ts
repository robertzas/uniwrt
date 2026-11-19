import { describe, expect, it } from "vitest";
import { blocklistsView, CATALOG } from "../../../test/blocklists";
import { siteConfig } from "../../../test/site";
import {
  blockingDraft,
  blockingErrorsOfIssues,
  blockingOf,
  estimate,
  isBusy,
  isListUrl,
  listState,
  parseDomains,
  syncStatus,
  withEnabled,
} from "./blocking";

const blocking = () => siteConfig().services.blocking;

describe("DNS filtering's settings", () => {
  it("round-trips the saved settings through the draft", () => {
    const saved = {
      ...blocking(),
      enabled: true,
      custom: [{ name: "Mine", url: "https://example.com/hosts.txt" }],
      allow: ["good.example.com"],
      deny: ["ads.example.com", "track.example.net"],
      bypass: ["3c:22:fb:12:34:56"],
    };
    expect(blockingOf(blockingDraft(saved)).blocking).toEqual(saved);
  });

  it("reads typed domains as the model wants them", () => {
    expect(parseDomains(" Ads.Example.com,\n*.track.example.net  cdn.example.org.\n\n")).toEqual([
      "ads.example.com",
      "track.example.net",
      "cdn.example.org",
    ]);
    const draft = { ...blockingDraft(blocking()), deny: "a.example.com\nA.example.com" };
    expect(blockingOf(draft).blocking?.deny).toEqual(["a.example.com"]);
  });

  it("refuses what the server refuses", () => {
    const draft = {
      ...blockingDraft(blocking()),
      custom: [
        { name: "", url: "ftp://example.com/x" },
        { name: "A", url: "https://example.com/a" },
        { name: "B", url: "https://example.com/a" },
      ],
      allow: "good.example.com",
      deny: "good.example.com",
    };
    const { blocking: none, errors } = blockingOf(draft);
    expect(none).toBeUndefined();
    expect(errors.custom[0]).toEqual({
      name: "Name the list in up to 64 characters",
      url: expect.stringContaining("http:// or https://"),
    });
    expect(errors.custom[1]).toEqual({});
    expect(errors.custom[2]).toEqual({ url: "This list is named twice" });
    expect(errors.deny).toBe("good.example.com is allowed too");
    expect(blockingOf({ ...draft, custom: [], deny: "bad_domain!" }).errors.deny).toBe(
      "bad_domain! is not a domain, such as ads.example.com",
    );
    const many = Array.from({ length: 1001 }, (_, i) => `d${i}.example.com`).join("\n");
    expect(blockingOf({ ...draft, custom: [], allow: many, deny: "" }).errors.allow).toBe(
      "At most 1,000 domains",
    );
  });

  it("checks list URLs as the model does", () => {
    expect(isListUrl("https://example.com/hosts.txt")).toBe(true);
    expect(isListUrl("http://example.com")).toBe(true);
    for (const bad of [
      "https://",
      "https:///x",
      "ftp://example.com",
      "https://a b",
      "https://a/'",
    ]) {
      expect(isListUrl(bad), bad).toBe(false);
    }
  });

  it("starts from the default lists when turned on with nothing to block", () => {
    const empty = { ...blockingDraft(blocking()), lists: [] };
    expect(withEnabled(empty, true).lists).toEqual(["hagezi_normal", "urlhaus"]);
    expect(withEnabled({ ...empty, deny: "x.example.com" }, true).lists).toEqual([]);
    expect(withEnabled(empty, false).lists).toEqual([]);
  });

  it("maps the server's issues to their fields", () => {
    const { errors, other } = blockingErrorsOfIssues([
      { path: "services/blocking/custom/1/url", message: "Bad URL" },
      { path: "services/blocking/custom", message: "At most 16" },
      { path: "services/blocking/deny/3", message: "Twice" },
      { path: "services/blocking/bypass/0", message: "Not a client" },
      { path: "services/blocking/lists/0", message: "No list is called x" },
    ]);
    expect(errors.custom[1]).toEqual({ url: "Bad URL" });
    expect(errors.customCount).toBe("At most 16");
    expect(errors.deny).toBe("Twice");
    expect(errors.bypass).toBe("Not a client");
    expect(other).toEqual(["No list is called x"]);
  });

  it("estimates the chosen lists' size", () => {
    expect(estimate(CATALOG, ["hagezi_normal", "urlhaus"])).toBe(303_000);
    expect(estimate(CATALOG, [])).toBe(0);
  });
});

describe("DNS filtering's state", () => {
  const sync = (state: Parameters<typeof blocklistsView>[0]) => syncStatus(blocklistsView(state));

  it("words where the gateway's copy stands", () => {
    const base = blocklistsView().sync;
    expect(sync({ sync: { ...base, state: "synced", domains: 123_456 } })).toEqual({
      tone: "ok",
      text: "The gateway blocks 123,456 domains",
    });
    expect(sync({ sync: { ...base, state: "pushing" } })).toMatchObject({
      tone: "info",
      pulse: true,
    });
    expect(sync({ sync: { ...base, state: "failed", error: "ssh: timeout" } })).toEqual({
      tone: "crit",
      text: "Sending the lists failed; trying again: ssh: timeout",
    });
    expect(sync({ sync: { ...base, state: "offline" } }).tone).toBe("warn");
    expect(sync({}).tone).toBe("neutral");
  });

  it("words a list's download", () => {
    const list = {
      id: "urlhaus",
      name: "URLhaus",
      url: "https://example.com/urlhaus.txt",
      checked_at: null,
      fetched_at: null,
      domains: null,
      error: null,
    };
    expect(listState(list)).toEqual({ tone: "neutral", text: "Not downloaded yet" });
    expect(listState({ ...list, error: "HTTP 404" })).toEqual({
      tone: "warn",
      text: "Download failed: HTTP 404",
    });
    expect(listState({ ...list, error: "HTTP 404", domains: 10 }).text).toBe(
      "Download failed: HTTP 404; the last good copy is used",
    );
    expect(listState({ ...list, domains: 2_500, fetched_at: 0 }).text).toMatch(
      /^2,500 domains · updated /,
    );
  });

  it("reads the state often only while it moves", () => {
    const base = blocklistsView();
    expect(isBusy(undefined)).toBe(false);
    expect(isBusy(base)).toBe(false);
    expect(isBusy({ ...base, refreshing: true })).toBe(true);
    expect(isBusy({ ...base, sync: { ...base.sync, state: "waiting" } })).toBe(true);
  });
});
