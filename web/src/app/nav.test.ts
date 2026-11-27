import { describe, expect, it } from "vitest";
import { helpTopic } from "../lib/help";
import { navItems, pageTitle } from "./nav";

describe("nav", () => {
  it("has a help section for every page in the sidebar (D-151)", () => {
    for (const item of navItems) expect(helpTopic(item.path)?.anchor, item.path).toBeDefined();
  });

  it("titles the help center", () => {
    expect(pageTitle("/help")).toBe("Help center");
    expect(pageTitle("/help/devices")).toBe("Help center");
  });
});
