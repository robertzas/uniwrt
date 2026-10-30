import { describe, expect, it, vi } from "vitest";
import { summary } from "../test/fixtures";
import { appCommands, filterCommands } from "./commands";

describe("commands", () => {
  const commands = appCommands({ navigate: vi.fn() });

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
    const [first] = filterCommands(appCommands({ navigate }), "updates");
    first?.run();
    expect(navigate).toHaveBeenCalledWith("/updates");
  });

  it("finds devices by name, MAC, address or model, and opens them", () => {
    const navigate = vi.fn();
    const devices = [
      summary(1, "gateway", { role: "gateway", mac: "aa:bb:cc:00:00:01", model: "GL-MT6000" }),
      summary(2, "attic", { host: "192.168.1.22" }),
    ];
    const all = appCommands({ navigate, devices });
    const find = (query: string) => filterCommands(all, query).map((c) => c.id);
    expect(find("attic")[0]).toBe("device:2");
    expect(find("aa:bb:cc")).toEqual(["device:1"]);
    expect(find("192.168.1.22")).toEqual(["device:2"]);
    expect(find("mt6000")).toEqual(["device:1"]);
    filterCommands(all, "attic")[0]?.run();
    expect(navigate).toHaveBeenCalledWith("/devices/2");
  });

  it("opens the adoption wizard", () => {
    const navigate = vi.fn();
    const [command] = filterCommands(appCommands({ navigate }), "adopt");
    expect(command?.id).toBe("devices:adopt");
    command?.run();
    expect(navigate).toHaveBeenCalledWith("/devices?adopt=1");
  });

  it("signs out when the app gives a way to", () => {
    const signOut = vi.fn();
    expect(appCommands({ navigate: vi.fn() }).map((c) => c.id)).not.toContain("account:sign-out");
    const [command] = filterCommands(appCommands({ navigate: vi.fn(), signOut }), "sign out");
    command?.run();
    expect(signOut).toHaveBeenCalledOnce();
  });
});
