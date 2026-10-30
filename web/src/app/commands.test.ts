import { describe, expect, it, vi } from "vitest";
import { appCommands, filterCommands } from "./commands";

describe("commands", () => {
  const commands = appCommands(vi.fn());

  it("lists every page, then the appearance commands", () => {
    expect(commands[0]).toMatchObject({ id: "go:/", label: "Dashboard", group: "Go to" });
    expect(commands.map((c) => c.id)).toContain("skin:glass");
    expect(commands.map((c) => c.id)).toContain("theme:dark");
  });

  it("matches every word, in labels, groups and keywords", () => {
    expect(filterCommands(commands, "").length).toBe(commands.length);
    expect(filterCommands(commands, "LOGS").map((c) => c.label)).toEqual(["Logs"]);
    // "roaming" appears only in descriptions.
    expect(filterCommands(commands, "roaming").map((c) => c.label)).toEqual(["Clients", "Wi-Fi"]);
    expect(filterCommands(commands, "glass skin").map((c) => c.id)).toEqual(["skin:glass"]);
    expect(filterCommands(commands, "nothing like this")).toEqual([]);
  });

  it("puts labels that start with the query first, keeping groups together", () => {
    const ids = filterCommands(commands, "dark").map((c) => c.id);
    expect(ids[0]).toBe("theme:dark");
    const groups = filterCommands(commands, "s").map((c) => c.group);
    const runs = groups.filter((g, i) => g !== groups[i - 1]);
    expect(new Set(runs).size).toBe(runs.length);
  });

  it("navigates to a page and switches skins", () => {
    const navigate = vi.fn();
    const [first] = filterCommands(appCommands(navigate), "updates");
    first?.run();
    expect(navigate).toHaveBeenCalledWith("/updates");
  });
});
