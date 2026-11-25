import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RfSettings } from "../../api/gen/RfSettings";
import { rfView } from "../../test/rf";
import { createRfSaver, type SaveRfSettings, SETTLE_MS } from "./settings";

describe("saveRfSettings", () => {
  let sent: RfSettings[];
  let saveRfSettings: SaveRfSettings;
  const shown = rfView().settings;

  beforeEach(() => {
    sent = [];
    saveRfSettings = createRfSaver();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init: RequestInit) => {
        const body = JSON.parse(String(init.body)) as RfSettings;
        sent.push(body);
        return Response.json(body);
      }),
    );
  });
  afterEach(() => {
    vi.runAllTimers();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("lays a second change over the first while the page still shows the old settings", async () => {
    // Regression (M9.1a3's RF e2e): the nightly start, then DFS, changed before the topic's push.
    const first = saveRfSettings(shown, { nightly_at_min: 120 });
    const second = saveRfSettings(shown, { dfs: "non_weather" });
    await Promise.all([first, second]);
    expect(sent).toEqual([
      { ...shown, nightly_at_min: 120 },
      { ...shown, nightly_at_min: 120, dfs: "non_weather" },
    ]);
  });

  it("bases a change on the settings shown again once the last save has settled", async () => {
    await saveRfSettings(shown, { interval_h: 8 });
    vi.advanceTimersByTime(SETTLE_MS);
    const elsewhere = { ...shown, interval_h: 12 };
    await saveRfSettings(elsewhere, { scanning: false });
    expect(sent.at(-1)).toEqual({ ...elsewhere, scanning: false });
  });

  it("forgets a failed save's settings", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(new Response("no", { status: 500 }));
    await expect(saveRfSettings(shown, { interval_h: 8 })).rejects.toThrow();
    await saveRfSettings(shown, { dfs: "avoid" });
    expect(sent.at(-1)).toEqual({ ...shown, dfs: "avoid" });
  });
});
