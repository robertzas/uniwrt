/**
 * A packet capture's form and file (D-142): the draft checked as `CaptureQuery::validate` checks
 * it, and a counter of the `.pcap`'s whole records, so that a capture stopped mid-packet saves a
 * valid file.
 */
import type { CaptureQuery } from "../../../api/gen/CaptureQuery";
import { strings } from "../../../lib/strings";
import { isIfname } from "./draft";

const e = strings.device.tools.capture.errors;

/** `tcpdump -i any`: every device. */
export const ANY = "any";
export const MAX_SECONDS = 3600;
export const MAX_MIB = 256;
export const MIN_SNAPLEN = 96;
export const MAX_SNAPLEN = 262_144;
const MAX_FILTER_LEN = 1024;

/** The form as typed. */
export interface CaptureDraft {
  interface: string;
  filter: string;
  seconds: string;
  maxMib: string;
  snaplen: string;
}

export type CaptureErrors = Partial<Record<keyof CaptureDraft, string>>;

export function emptyCapture(): CaptureDraft {
  return { interface: ANY, filter: "", seconds: "60", maxMib: "16", snaplen: "" };
}

/** Why the controller would refuse the filter, if it would (the model's `check_filter`). */
export function filterError(filter: string): string | undefined {
  const text = filter.trim();
  if (text.length > MAX_FILTER_LEN) return e.filterLong;
  if (!/^[\x20-\x7e]*$/.test(text)) return e.filterAscii;
  if (text.startsWith("-")) return e.filterDash;
  let depth = 0;
  for (const char of text) {
    if (char === "(") depth += 1;
    if (char === ")") {
      depth -= 1;
      if (depth < 0) return e.filterClose;
    }
  }
  return depth > 0 ? e.filterOpen : undefined;
}

function whole(text: string, min: number, max: number): number | undefined {
  if (!/^\d+$/.test(text.trim())) return undefined;
  const value = Number(text.trim());
  return value >= min && value <= max ? value : undefined;
}

export function captureErrors(draft: CaptureDraft): CaptureErrors {
  const errors: CaptureErrors = {};
  const device = draft.interface.trim();
  if (device !== ANY && !isIfname(device)) errors.interface = e.interface;
  const filter = filterError(draft.filter);
  if (filter) errors.filter = filter;
  if (whole(draft.seconds, 1, MAX_SECONDS) === undefined) errors.seconds = e.seconds;
  if (whole(draft.maxMib, 1, MAX_MIB) === undefined) errors.maxMib = e.maxMib;
  if (draft.snaplen.trim() && whole(draft.snaplen, MIN_SNAPLEN, MAX_SNAPLEN) === undefined) {
    errors.snaplen = e.snaplen;
  }
  return errors;
}

/** The query the draft asks, once it is valid. */
export function toCaptureQuery(draft: CaptureDraft): CaptureQuery | undefined {
  if (Object.keys(captureErrors(draft)).length > 0) return undefined;
  const filter = draft.filter.trim();
  return {
    interface: draft.interface.trim(),
    filter: filter || null,
    seconds: Number(draft.seconds.trim()),
    max_mib: Number(draft.maxMib.trim()),
    snaplen: draft.snaplen.trim() ? Number(draft.snaplen.trim()) : null,
  };
}

const MAGICS = [0xa1b2c3d4, 0xa1b23c4d]; // microsecond and nanosecond timestamps
const FILE_HEADER = 24;
const RECORD_HEADER = 16;

/**
 * Counts a `.pcap` stream's packets as its chunks arrive, and how many of its bytes are the file
 * header and whole records. A stream that isn't a `.pcap` counts as whole.
 */
export class PcapCounter {
  packets = 0;
  #complete = 0;
  #total = 0;
  #carry = new Uint8Array(0);
  #little: boolean | undefined;
  #opaque = false;

  push(chunk: Uint8Array) {
    this.#total += chunk.length;
    if (this.#opaque) return;
    let buffer = chunk;
    if (this.#carry.length > 0) {
      buffer = new Uint8Array(this.#carry.length + chunk.length);
      buffer.set(this.#carry);
      buffer.set(chunk, this.#carry.length);
    }
    const view = new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    let offset = 0;
    if (this.#little === undefined) {
      if (buffer.length < FILE_HEADER) {
        this.#carry = buffer.slice();
        return;
      }
      if (MAGICS.includes(view.getUint32(0, true))) this.#little = true;
      else if (MAGICS.includes(view.getUint32(0, false))) this.#little = false;
      else {
        this.#opaque = true;
        return;
      }
      offset = FILE_HEADER;
      this.#complete = FILE_HEADER;
    }
    while (buffer.length - offset >= RECORD_HEADER) {
      const length = RECORD_HEADER + view.getUint32(offset + 8, this.#little);
      if (buffer.length - offset < length) break;
      offset += length;
      this.#complete += length;
      this.packets += 1;
    }
    this.#carry = buffer.slice(offset);
  }

  /** The bytes worth keeping: the header and every whole record. */
  get complete(): number {
    return this.#opaque ? this.#total : this.#complete;
  }
}
