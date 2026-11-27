import { chapters } from "virtual:uniwrt-guide";
import { describe, expect, it } from "vitest";
import { helpHref, helpTopic, helpTopics, parseHelpHref } from "./help";

describe("help topics", () => {
  it("names a page's section, its tab's, or its section's under a path", () => {
    expect(helpTopic("/")).toEqual({ chapter: "dashboard-topology", anchor: "dashboard" });
    expect(helpTopic("/devices/")).toEqual({ chapter: "devices", anchor: "devices" });
    expect(helpTopic("/devices/7")).toEqual({ chapter: "devices", anchor: "device-page" });
    expect(helpTopic("/devices/7", "?tab=uci")).toEqual({ chapter: "devices", anchor: "uci" });
    expect(helpTopic("/devices/7", "?tab=nope")).toEqual({
      chapter: "devices",
      anchor: "device-page",
    });
    expect(helpTopic("/settings", "?tab=backups")).toEqual({
      chapter: "settings",
      anchor: "backups",
    });
    expect(helpTopic("/settings/import")).toEqual({
      chapter: "changes",
      anchor: "import-from-the-devices",
    });
    expect(helpTopic("/somewhere")).toEqual({ chapter: "index" });
    expect(helpTopic("/help/devices")).toBeUndefined();
  });

  it("points only at sections the guide has", () => {
    const anchors = new Map(chapters.map((c) => [c.slug, c.sections.map((s) => s.anchor)]));
    for (const topic of helpTopics()) {
      expect(anchors.get(topic.chapter), topic.chapter).toContain(topic.anchor);
    }
  });

  it("writes and reads help links", () => {
    expect(helpHref({ chapter: "index" })).toBe("/help");
    expect(helpHref({ chapter: "wifi", anchor: "roaming" })).toBe("/help/wifi#roaming");
    expect(parseHelpHref("/help")).toEqual({ chapter: "index" });
    expect(parseHelpHref("/help/wifi#roaming")).toEqual({ chapter: "wifi", anchor: "roaming" });
    expect(parseHelpHref("/helpers")).toBeUndefined();
    expect(parseHelpHref("https://example.com/help")).toBeUndefined();
  });
});
