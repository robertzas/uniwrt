/**
 * RF insight live and in words (features/wifi.md, RF insight): the WS topic `rf` (the report,
 * each radio's scanning, the settings) while the calling component lives, and how its numbers
 * read. Shared by the RF page and the dashboard.
 */
import { createEffect, createSignal } from "solid-js";
import type { Band } from "../api/gen/Band";
import type { BandReport } from "../api/gen/BandReport";
import type { RfView } from "../api/gen/RfView";
import { formatDuration } from "./format";
import type { Live } from "./live";
import { strings } from "./strings";

/** A badge's tone (ui/Badge's `Tone`, which lib/ may not import). */
export type Tone = "ok" | "warn" | "crit" | "neutral";

export const TOPIC = "rf";

/** The view over `live`'s socket while the calling component lives; undefined until it comes. */
export function createRfView(live: Live): () => RfView | undefined {
  const [view, setView] = createSignal<RfView>();
  createEffect(
    () => TOPIC,
    (topic) =>
      live.socket.subscribe(topic, (message) => {
        if (message.type === "rf") setView(() => message.view);
      }),
  );
  return view;
}

/** The bands the site uses, in band order. */
export const BANDS: readonly Band[] = ["2g", "5g", "6g"];

export function bandOf(view: RfView | undefined, band: Band): BandReport | undefined {
  return view?.report.bands.find((b) => b.band === band);
}

/** How crowded a radio's channel is, from its interference (the sum of overlapping impacts). */
export type Crowding = "clean" | "some" | "crowded";

export function crowding(interference: number): Crowding {
  if (interference < 0.3) return "clean";
  if (interference < 1) return "some";
  return "crowded";
}

export const CROWDING_TONE: Record<Crowding, Tone> = {
  clean: "ok",
  some: "warn",
  crowded: "crit",
};

/** How busy a channel is: under 50 % fine, under 75 % busy, else congested. */
export function busyTone(percent: number | null | undefined): Tone {
  if (percent == null) return "neutral";
  if (percent < 50) return "ok";
  if (percent < 75) return "warn";
  return "crit";
}

/** How well one node hears another: good from −65 dBm, fair from −75, else weak. */
export function linkTone(signal: number): Tone {
  if (signal >= -65) return "ok";
  if (signal >= -75) return "warn";
  return "crit";
}

/** The site's networks first heard best by its radios: the strongest foreign ones on a band. */
export function strongestForeign(band: BandReport, count: number) {
  return band.networks.filter((n) => n.kind !== "own").slice(0, count);
}

/** `at` (ms) from `now`, in words: "2 h ago", "in 40 min", "just now". */
export function relative(at: number, now: number): string {
  const seconds = Math.round((at - now) / 1000);
  if (Math.abs(seconds) < 60) return strings.rf.justNow;
  const words = formatDuration(Math.abs(seconds));
  return seconds > 0 ? strings.rf.in(words) : strings.rf.ago(words);
}
