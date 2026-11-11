import { describe, expect, it } from "vitest";
import type { Wlan } from "../../api/gen/Wlan";
import { siteConfig } from "../../test/site";
import {
  draftOf,
  errorsOfIssues,
  idOf,
  joinCode,
  minutesOf,
  presetWlan,
  withWlan,
  wlanOf,
} from "./draft";
import { qrPath } from "./JoinDialog";
import { daysText, isOn, localTime, onNow, scopeText, spanText } from "./summary";

const home = (): Wlan => siteConfig().wlans[0] as Wlan;

describe("WLAN drafts", () => {
  it("round-trips a WLAN and keeps its passphrase reference", () => {
    const wlan: Wlan = {
      ...home(),
      max_clients: 30,
      nodes: { scope: "only", nodes: [2, 1] },
      schedule: { spans: [{ days: ["mon", "fri"], from: 420, to: 1320 }] },
    };
    const out = wlanOf(draftOf(wlan), wlan, []);
    expect(out.errors).toEqual({});
    expect(out.wlan).toEqual({ ...wlan, nodes: { scope: "only", nodes: [1, 2] } });
    expect(out.secrets).toBeUndefined();
  });

  it("sends a new passphrase as a secret and drops the reference of an open WLAN", () => {
    const wlan = home();
    const changed = wlanOf({ ...draftOf(wlan), passphrase: "correct horse" }, wlan, []);
    expect(changed.secrets).toEqual({ "wlans/home/passphrase": "correct horse" });
    expect(changed.wlan?.passphrase).toBe(wlan.passphrase);
    const open = wlanOf({ ...draftOf(wlan), security: "owe_transition" }, wlan, []);
    expect(open.wlan).toMatchObject({ passphrase: null, security: "owe_transition" });
    expect(open.secrets).toBeUndefined();
  });

  it("names what is wrong", () => {
    const wlan = presetWlan("main", siteConfig());
    const draft = {
      ...draftOf(wlan),
      ssid: "",
      bands: [],
      scope: "only" as const,
      maxClients: "0",
      scheduled: true,
      spans: [],
    };
    expect(Object.keys(wlanOf(draft, wlan, []).errors).sort()).toEqual([
      "bands",
      "maxClients",
      "nodes",
      "passphrase",
      "schedule",
      "ssid",
    ]);
    const six = { ...draftOf(home()), bands: ["5g" as const, "6g" as const] };
    expect(wlanOf(six, home(), []).errors.bands).toMatch(/6 GHz requires WPA3 or OWE/);
    const taken = wlanOf({ ...draftOf(wlan), ssid: "uniwrt-demo", passphrase: "12345678" }, wlan, [
      home(),
    ]);
    expect(taken.errors.ssid).toBe("Another WLAN has this name.");
    const long = {
      ...draftOf(home()),
      ssid: "x".repeat(30),
      bands: ["2g" as const, "6g" as const],
    };
    expect(wlanOf({ ...long, security: "owe_transition" }, home(), []).errors).toEqual({
      ssid: expect.stringMatching(/29 bytes/),
    });
    expect(wlanOf({ ...long, security: "owe" }, home(), []).errors).toEqual({});
    const days = { ...draftOf(home()), scheduled: true };
    expect(
      wlanOf({ ...days, spans: [{ days: [], from: "07:00", to: "08:00" }] }, home(), []).errors
        .schedule,
    ).toMatch(/one day/);
    expect(
      wlanOf({ ...days, spans: [{ days: ["mon"], from: "7", to: "08:00" }] }, home(), []).errors
        .schedule,
    ).toBe("Times are HH:MM.");
  });

  it("starts new WLANs from presets and derives their ids", () => {
    const config = siteConfig();
    config.networks.push({
      ...(config.networks[0] as (typeof config.networks)[0]),
      id: "guest",
      purpose: "guest",
    });
    expect(presetWlan("guest", config)).toMatchObject({
      network: "guest",
      security: "owe_transition",
      isolation: true,
    });
    expect(presetWlan("iot", config)).toMatchObject({
      network: "lan",
      security: "wpa2",
      bands: ["2g"],
    });
    expect(idOf("Guest Wi-Fi", [])).toBe("guest_wi_fi");
    expect(idOf("5 Zimmer Küche Bad!", [])).toBe("zimmer_k_che");
    expect(idOf("Home", ["home", "home2"])).toBe("home3");
    expect(idOf("日本", [])).toBe("wlan");
    const added = wlanOf(
      { ...draftOf(presetWlan("main", config)), ssid: "Home", passphrase: "12345678" },
      presetWlan("main", config),
      [{ ...home(), id: "home" }],
    );
    expect(added.wlan?.id).toBe("home2");
    expect(added.secrets).toEqual({ "wlans/home2/passphrase": "12345678" });
  });

  it("edits the site's list", () => {
    const config = siteConfig();
    const guest = { ...home(), id: "guest", ssid: "Guest" };
    const added = withWlan(config, guest);
    expect(added.wlans.map((w) => w.id)).toEqual(["home", "guest"]);
    expect(withWlan(added, { ...guest, ssid: "Visitors" }).wlans[1]?.ssid).toBe("Visitors");
    expect(withWlan(added, { remove: "home" }).wlans.map((w) => w.id)).toEqual(["guest"]);
  });

  it("puts the server's errors on their fields", () => {
    const mapped = errorsOfIssues(
      [
        { path: "wlans/home/security", severity: "error", message: "6 GHz requires WPA3" },
        {
          path: "wlans/home/schedule/spans/0",
          severity: "error",
          message: "A day is listed twice",
        },
        { path: "wlans/other/ssid", severity: "error", message: "Elsewhere" },
      ],
      "home",
    );
    expect(mapped.errors).toEqual({
      bands: "6 GHz requires WPA3",
      schedule: "A day is listed twice",
    });
    expect(mapped.other).toEqual(["Elsewhere"]);
  });

  it("writes join codes cameras read", () => {
    expect(joinCode(home(), 'p;a"s\\s')).toBe('WIFI:T:WPA;S:uniwrt-demo;P:p\\;a\\"s\\\\s;;');
    expect(joinCode({ ...home(), security: "wpa3", hidden: true }, "x")).toBe(
      "WIFI:T:SAE;S:uniwrt-demo;P:x;H:true;;",
    );
    expect(joinCode({ ...home(), security: "owe" }, undefined)).toBe(
      "WIFI:T:nopass;S:uniwrt-demo;;",
    );
    const qr = qrPath("WIFI:T:nopass;S:uniwrt-demo;;");
    expect(qr.size).toBeGreaterThan(21);
    expect(qr.d).toMatch(/^M\d+ \d+h1v1h-1z/);
  });
});

describe("WLAN summaries", () => {
  it("words days, spans and scopes", () => {
    expect(daysText(["mon", "tue", "wed", "thu", "fri"])).toBe("Mon–Fri");
    expect(daysText(["sat", "sun"])).toBe("Sat, Sun");
    expect(daysText(["mon", "wed", "thu", "fri", "sun"])).toBe("Mon, Wed–Fri, Sun");
    expect(daysText(["mon", "tue", "wed", "thu", "fri", "sat", "sun"])).toBe("Every day");
    expect(spanText({ days: ["mon"], from: 420, to: 1320 })).toBe("Mon 07:00–22:00");
    expect(spanText({ days: ["sat", "sun"], from: 0, to: 0 })).toBe("Sat, Sun all day");
    expect(minutesOf("23:59")).toBe(1439);
    expect(minutesOf("24:00")).toBeUndefined();
    const name = (id: number) => `ap${id}`;
    expect(scopeText(home(), name)).toBe("All devices");
    expect(scopeText({ ...home(), nodes: { scope: "only", nodes: [2, 3] } }, name)).toBe(
      "Only ap2, ap3",
    );
    expect(scopeText({ ...home(), nodes: { scope: "except", nodes: [1] } }, name)).toBe(
      "All but ap1",
    );
  });

  it("tells whether a schedule is on, as the model does", () => {
    const night = { spans: [{ days: ["fri" as const], from: 22 * 60, to: 6 * 60 }] };
    expect(isOn(night, "fri", 23 * 60)).toBe(true);
    expect(isOn(night, "sat", 5 * 60 + 59)).toBe(true);
    expect(isOn(night, "sat", 6 * 60)).toBe(false);
    expect(isOn(night, "fri", 5 * 60)).toBe(false);
    const whole = { spans: [{ days: ["mon" as const], from: 60, to: 60 }] };
    expect(isOn(whole, "mon", 1439)).toBe(true);
    expect(isOn(whole, "tue", 59)).toBe(true);
    expect(isOn(whole, "tue", 60)).toBe(false);
    // 2026-11-13 is a Friday; 22:30 UTC is 23:30 in Berlin and 17:30 in New York.
    const at = new Date("2026-11-13T22:30:00Z");
    expect(localTime("Europe/Berlin", at)).toEqual({ day: "fri", minute: 23 * 60 + 30 });
    expect(localTime("America/New_York", at)).toEqual({ day: "fri", minute: 17 * 60 + 30 });
    expect(localTime("Mars/Olympus", at)).toBeUndefined();
    expect(onNow(night, "Europe/Berlin", at)).toBe(true);
    expect(onNow(night, "America/New_York", at)).toBe(false);
  });
});
