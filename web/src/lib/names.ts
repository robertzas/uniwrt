/** Names as the site document checks them (`uniwrt_model`'s `validate/names.rs`). */

/** A host name: one DNS label (RFC 1123). */
export function isHostname(name: string): boolean {
  return /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(name);
}

/** A DNS name: dot-separated host names, up to 253 bytes. */
export function isDnsName(name: string): boolean {
  return name.length <= 253 && name.split(".").every(isHostname);
}

/** A MAC address as the API writes it (lowercase, colons), or `undefined`. */
export function normalizeMac(text: string): string | undefined {
  const mac = text.trim().toLowerCase().replaceAll("-", ":");
  return /^([0-9a-f]{2}:){5}[0-9a-f]{2}$/.test(mac) ? mac : undefined;
}

/** A client's name made a host name, when it can be: `Anna's iPhone` → `annas-iphone`. */
export function hostnameOf(name: string | null): string {
  const label = (name ?? "")
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z0-9 -]/g, "")
    .trim()
    .replace(/[\s-]+/g, "-")
    .slice(0, 63)
    .replace(/^-+|-+$/g, "");
  return isHostname(label) ? label : "";
}
