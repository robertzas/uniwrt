import type { RawSection } from "../../../api/gen/RawSection";
import type { RawValue } from "../../../api/gen/RawValue";
import type { UciTreePackage } from "../../../api/gen/UciTreePackage";
import type { UciTreeSection } from "../../../api/gen/UciTreeSection";
import type { Value } from "../../../api/gen/Value";

/** The options render knows hold secrets (`model::site::node::SECRET_OPTIONS`). */
const SECRET_OPTIONS = new Set([
  "key",
  "key1",
  "key2",
  "key3",
  "key4",
  "password",
  "sae_password",
  "auth_secret",
  "acct_secret",
  "dae_secret",
  "priv_key_pwd",
  "private_key",
  "preshared_key",
  "r0kh",
  "r1kh",
]);
const SECRET_WORDS = [
  "passw",
  "passphrase",
  "secret",
  "psk",
  "token",
  "private",
  "credential",
  "apikey",
  "api_key",
];

/**
 * Whether the explorer masks an option of that name, as the controller does
 * (`model::explorer::is_masked_option`): an option it edits is then sealed in the vault.
 */
export function isMaskedOption(name: string): boolean {
  const lower = name.toLowerCase();
  return SECRET_OPTIONS.has(name) || SECRET_WORDS.some((word) => lower.includes(word));
}

/** A UCI option or section name: letters, digits and `_`. */
export const isUciName = (name: string): boolean => /^[A-Za-z0-9_]+$/.test(name);
/** A section type: printable ASCII without spaces, such as `wifi-iface`. */
export const isUciType = (name: string): boolean => /^[\x21-\x7e]+$/.test(name);

/** How `uci` addresses a section: its name, or `@type[n]` for an anonymous one. */
export function sectionLabel(pkg: UciTreePackage, index: number): string {
  const section = pkg.sections[index];
  if (!section) return "";
  if (section.name !== null) return section.name;
  const nth = pkg.sections.slice(0, index).filter((s) => s.type === section.type).length;
  return `@${section.type}[${nth}]`;
}

/** Two values alike: the same string, or lists with the same items in order. */
export function sameValue(a: RawValue | Value | undefined, b: Value): boolean {
  if (typeof a === "string" || typeof b === "string") return a === b;
  return Array.isArray(a) && a.length === b.length && a.every((item, i) => item === b[i]);
}

/** Whether `section` holds `name` as `value` (a plain one: a masked option never matches). */
export function holds(section: UciTreeSection, name: string, value: RawValue): boolean {
  if (typeof value !== "string" && !Array.isArray(value)) return false;
  const option = section.options.find((o) => o.name === name);
  return option !== undefined && !option.secret && sameValue(value, option.value);
}

/** The words of a search, lowercased. */
export function searchWords(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** Whether every word is in the section's type, name, option names or (plain) values. */
export function sectionMatches(
  pkg: UciTreePackage,
  index: number,
  words: readonly string[],
): boolean {
  if (words.length === 0) return true;
  const section = pkg.sections[index];
  if (!section) return false;
  const text = [
    pkg.name,
    section.type,
    sectionLabel(pkg, index),
    ...section.options.flatMap((o) => [o.name, ...(o.secret ? [] : [o.value].flat())]),
  ]
    .join("\n")
    .toLowerCase();
  return words.every((word) => text.includes(word));
}

/** The indices of the package's sections that match the search. */
export function matchingSections(pkg: UciTreePackage, words: readonly string[]): number[] {
  return pkg.sections.flatMap((_, i) => (sectionMatches(pkg, i, words) ? [i] : []));
}

/** What a raw section does: adds or replaces a whole section, merges into one, or removes one. */
export function rawKind(raw: RawSection): "whole" | "merged" | "removed" {
  return raw.remove ? "removed" : raw.merge ? "merged" : "whole";
}

/** A raw section's target as `uci` would name it: `package.name`, else `package.@type`. */
export function rawTarget(raw: RawSection): string {
  return `${raw.package}.${raw.name ?? `@${raw.type}`}`;
}

/** A raw value as text: a list's items joined, a sealed secret masked. */
export function rawText(value: RawValue): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.join(" · ");
  return "••••••••";
}
