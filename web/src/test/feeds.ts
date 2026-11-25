import type { FeedsView } from "../api/gen/FeedsView";
import type { SetStatus } from "../api/gen/SetStatus";

const H = 3_600_000;
const AT = Date.UTC(2026, 10, 24, 9, 0);

/** A feed set of 25.12.6 on x86/64 run by the fleet, its feeds healthy. */
export function feedSet(extra: Partial<SetStatus> = {}): SetStatus {
  const base = "https://downloads.openwrt.org/releases/25.12.6";
  return {
    set: { release: "25.12.6", target: "x86/64", arch: "x86_64", manager: "apk" },
    feeds: [
      {
        url: `${base}/targets/x86/64/packages`,
        index_at: AT - H,
        checked_at: AT,
        error: null,
        listed: 410,
        kept: 12,
        bytes: 3_200_000,
      },
      {
        url: `${base}/packages/x86_64/base`,
        index_at: AT - H,
        checked_at: AT,
        error: null,
        listed: 1220,
        kept: 30,
        bytes: 8_400_000,
      },
    ],
    kept: 42,
    bytes: 11_600_000,
    missing: [],
    failed: [],
    offered: false,
    ...extra,
  };
}

/** The mirror on, one healthy set, refreshed at 09:00 UTC on 2026-11-24. */
export function feedsView(extra: Partial<FeedsView> = {}): FeedsView {
  return {
    settings: { enabled: true, whole_feeds: false },
    sets: [feedSet()],
    images: [],
    bytes: 11_600_000,
    synced_at: AT,
    sync_error: null,
    syncing: false,
    ...extra,
  };
}
