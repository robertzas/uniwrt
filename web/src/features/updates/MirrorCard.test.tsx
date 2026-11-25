import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FeedsView } from "../../api/gen/FeedsView";
import { formatBytes } from "../../lib/format";
import { feedSet, feedsView } from "../../test/feeds";
import { MirrorCard } from "./MirrorCard";
import { feedName, MIRROR_POLL_MS, orderedSets } from "./mirror";

/** Answers `v1/feeds` with what `answers` returns, call by call; records the calls. */
function stubFeeds(answers: (key: string, body: unknown) => Response | FeedsView) {
  const calls: [string, unknown][] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const key = `${init?.method ?? "GET"} ${url}`;
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
      calls.push([key, body]);
      const answer = answers(key, body);
      return answer instanceof Response ? answer : Response.json(answer);
    }),
  );
  return calls;
}

const base = "https://downloads.openwrt.org/releases/24.10.8";

/** 24.10.8 in use with a failing feed, a missing name and a failed package; 25.12.6 on offer. */
const troubled = feedsView({
  sets: [
    feedSet({ offered: true }),
    feedSet({
      set: { release: "24.10.8", target: "x86/64", arch: "x86_64", manager: "opkg" },
      feeds: [
        {
          url: `${base}/packages/x86_64/routing`,
          index_at: null,
          checked_at: null,
          error: "downloads.openwrt.org: no route to host",
          listed: 0,
          kept: 0,
          bytes: 0,
        },
      ],
      kept: 0,
      bytes: 0,
      missing: ["tcpdump-mini"],
      failed: ["kmod-foo_1_x86_64.ipk: its hash isn't the index's"],
    }),
  ],
  images: [
    {
      url: "https://downloads.openwrt.org/releases/25.12.6/targets/x86/64/openwrt-25.12.6-x86-64-generic-squashfs-combined.img.gz",
      name: "openwrt-25.12.6-x86-64-generic-squashfs-combined.img.gz",
      bytes: 12_000_000,
      kept: true,
      error: null,
    },
  ],
  sync_error: "1 feed failed",
});

describe("feed mirror helpers", () => {
  it("names feeds by their place in OpenWrt's tree", () => {
    expect(feedName(`${base}/targets/x86/64/packages`)).toBe("core");
    expect(feedName(`${base}/targets/x86/64/kmods/6.6.93-1-abc`)).toBe("kmods");
    expect(feedName(`${base}/packages/x86_64/luci`)).toBe("luci");
    expect(feedName(`${base}/packages/x86_64/base/packages.adb`)).toBe("base");
    expect(feedName("https://feeds.example/mine/")).toBe("mine");
  });

  it("puts the sets in use first, newest release first", () => {
    const sets = orderedSets([
      feedSet({ offered: true }),
      feedSet({ set: { release: "24.10.8", target: "a", arch: "b", manager: "opkg" } }),
      feedSet({ set: { release: "24.10.10", target: "a", arch: "b", manager: "opkg" } }),
    ]);
    expect(sets.map((s) => [s.set.release, s.offered])).toEqual([
      ["24.10.10", false],
      ["24.10.8", false],
      ["25.12.6", true],
    ]);
  });
});

describe("MirrorCard", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("shows each set, its feeds, what failed and the images kept", async () => {
    stubFeeds(() => troubled);
    render(() => <MirrorCard admin={false} />);
    const card = await screen.findByRole("region", { name: "Offline packages" });
    expect(await within(card).findByText("On, keeping what the fleet uses")).toBeTruthy();
    expect(within(card).getByText(`${formatBytes(11_600_000)} kept`)).toBeTruthy();
    expect(within(card).getByText("1 feed failed")).toBeTruthy();
    const summaries = card.querySelectorAll("summary");
    expect([...summaries].map((s) => s.textContent)).toEqual([
      expect.stringContaining("24.10.8 · x86/64 · x86_64 · opkg"),
      expect.stringContaining("25.12.6 · x86/64 · x86_64 · apk"),
    ]);
    expect(summaries[0]?.textContent).toContain("1 feed failing");
    expect(summaries[1]?.textContent).toContain("Upgrade on offer");
    const [failing, offered] = [...card.querySelectorAll("details")];
    expect(failing?.open).toBe(true);
    expect(offered?.open).toBe(false);
    const feeds = within(card).getByRole("table", { name: "24.10.8 · x86/64 · x86_64 · opkg" });
    const row = within(feeds).getAllByRole("row")[1] as HTMLElement;
    expect(within(row).getByText("routing")).toBeTruthy();
    expect(within(row).getByText("None yet")).toBeTruthy();
    expect(within(row).getByText("downloads.openwrt.org: no route to host")).toBeTruthy();
    expect(within(card).getByText("Not in any feed: tcpdump-mini")).toBeTruthy();
    expect(within(card).getByText(/its hash isn't the index's/)).toBeTruthy();
    const images = within(card).getByRole("table", { name: "Official images kept" });
    expect(within(images).getByText("Kept")).toBeTruthy();
    expect(within(card).queryByRole("checkbox")).toBeNull();
    expect(within(card).queryByRole("button", { name: "Refresh now" })).toBeNull();
  });

  it("turns the mirror off, and keeps whole feeds only once confirmed", async () => {
    const user = userEvent.setup();
    let settings = { enabled: true, whole_feeds: false };
    const calls = stubFeeds((key, body) => {
      if (key === "PUT /api/v1/feeds/settings") settings = body as typeof settings;
      return feedsView({ settings });
    });
    render(() => <MirrorCard admin />);
    const card = await screen.findByRole("region", { name: "Offline packages" });
    await user.click(await within(card).findByRole("checkbox", { name: "Keep whole feeds" }));
    const dialog = await screen.findByRole("dialog", { name: "Keep whole feeds?" });
    expect(dialog.textContent).toContain("about 3 GB for each of the 1 release and architecture");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.filter(([key]) => key.startsWith("PUT"))).toEqual([]);
    expect(within(card).getByRole("checkbox", { name: "Keep whole feeds" })).not.toBeChecked();

    await user.click(within(card).getByRole("checkbox", { name: "Keep whole feeds" }));
    const again = await screen.findByRole("dialog", { name: "Keep whole feeds?" });
    await user.click(within(again).getByRole("button", { name: "Keep whole feeds" }));
    await waitFor(() =>
      expect(calls).toContainEqual([
        "PUT /api/v1/feeds/settings",
        { enabled: true, whole_feeds: true },
      ]),
    );
    await waitFor(() =>
      expect(within(card).getByRole("checkbox", { name: "Keep whole feeds" })).toBeChecked(),
    );

    await user.click(within(card).getByRole("checkbox", { name: "Serve packages to devices" }));
    await waitFor(() =>
      expect(calls).toContainEqual([
        "PUT /api/v1/feeds/settings",
        { enabled: false, whole_feeds: true },
      ]),
    );
    expect(await within(card).findByText(/Off: devices fetch packages/)).toBeTruthy();
    expect(within(card).getByRole("checkbox", { name: "Keep whole feeds" })).toBeDisabled();
    expect(within(card).queryByRole("button", { name: "Refresh now" })).toBeNull();
  });

  it("starts a refresh and follows the sync until it ends", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    let reads = 0;
    const calls = stubFeeds((key) => {
      if (key === "POST /api/v1/feeds/refresh")
        return new Response(JSON.stringify(feedsView()), {
          status: 202,
          headers: { "content-type": "application/json" },
        });
      reads += 1;
      if (reads === 1) return feedsView({ synced_at: null, sets: [] });
      if (reads === 2) return feedsView({ syncing: true });
      return feedsView();
    });
    render(() => <MirrorCard admin />);
    const card = await screen.findByRole("region", { name: "Offline packages" });
    expect(await within(card).findByText("Not refreshed yet")).toBeTruthy();
    expect(within(card).getByText(/No release to keep yet/)).toBeTruthy();
    await user.click(within(card).getByRole("button", { name: "Refresh now" }));
    expect(calls.map(([key]) => key)).toContain("POST /api/v1/feeds/refresh");
    expect(await within(card).findByText("Refreshing the mirror…")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Refresh now" })).toBeDisabled();
    await vi.advanceTimersByTimeAsync(MIRROR_POLL_MS);
    expect(reads).toBe(2);
    await vi.advanceTimersByTimeAsync(MIRROR_POLL_MS);
    expect(await within(card).findByText(/^Refreshed /)).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Refresh now" })).toBeEnabled();
    const after = reads;
    await vi.advanceTimersByTimeAsync(MIRROR_POLL_MS * 3);
    expect(reads).toBe(after);
  });

  it("says why the mirror can't be read", async () => {
    stubFeeds(() => Response.json({ error: "the feed mirror isn't running" }, { status: 503 }));
    render(() => <MirrorCard admin />);
    const card = await screen.findByRole("region", { name: "Offline packages" });
    expect(await within(card).findByRole("alert")).toBeTruthy();
  });
});
