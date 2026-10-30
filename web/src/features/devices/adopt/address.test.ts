import { describe, expect, it } from "vitest";
import { formatAddress, parseAddress } from "./address";

describe("parseAddress", () => {
  it("takes host names and addresses, with an optional port", () => {
    expect(parseAddress(" 192.168.1.2 ")).toEqual({ host: "192.168.1.2" });
    expect(parseAddress("ap-attic.lan")).toEqual({ host: "ap-attic.lan" });
    expect(parseAddress("10.0.0.2:2222")).toEqual({ host: "10.0.0.2", port: 2222 });
    expect(parseAddress("10.0.0.2:22")).toEqual({ host: "10.0.0.2" });
    expect(parseAddress("fe80::1%br-lan")).toEqual({ host: "fe80::1%br-lan" });
    expect(parseAddress("[2001:db8::1]:2222")).toEqual({ host: "2001:db8::1", port: 2222 });
  });

  it("refuses what the server would", () => {
    for (const bad of [
      "",
      "  ",
      "-oProxyCommand=x",
      "a b",
      "host;id",
      "h:0",
      "h:65536",
      "[::1]:x",
    ]) {
      expect(parseAddress(bad), bad).toBeUndefined();
    }
  });

  it("writes addresses back as typed", () => {
    expect(formatAddress("10.0.0.2")).toBe("10.0.0.2");
    expect(formatAddress("10.0.0.2", 2222)).toBe("10.0.0.2:2222");
    expect(formatAddress("2001:db8::1", 2222)).toBe("[2001:db8::1]:2222");
  });
});
