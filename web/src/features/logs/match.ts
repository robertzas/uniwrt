/**
 * Whether a live entry belongs to the page's results: the hub narrows the `logs` topic only by
 * source and severity, so the page applies the rest of the filter as the store does
 * (`crates/store/src/repo/logs.rs`), full text included.
 */
import type { LogRecord } from "../../api/gen/LogRecord";
import type { Filter } from "./query";
import { severityCode } from "./severity";

/** As the store's: more terms than this are ignored. */
const MAX_TERMS = 16;

/**
 * Tokens as FTS5's `unicode61` tokenizer makes them: runs of letters and digits, folded to lower
 * case and stripped of diacritics.
 */
export function tokens(text: string): string[] {
  return (
    text
      .normalize("NFKD")
      .replace(/\p{M}/gu, "")
      .toLowerCase()
      .match(/[\p{L}\p{N}\p{Co}]+/gu) ?? []
  );
}

/** A phrase to find: its tokens, in order, the last one a prefix for `word*`. */
export interface Term {
  tokens: string[];
  prefix: boolean;
  negated: boolean;
}

/**
 * The terms of a text filter, read as the store reads them: words, `"phrases"`, `word*` prefixes
 * and `-word` exclusions. Words without a letter or digit are skipped.
 */
export function parseText(text: string): Term[] {
  const terms: Term[] = [];
  let rest = text.trimStart();
  while (rest.length > 0 && terms.length < MAX_TERMS) {
    const negated = rest.startsWith("-");
    if (negated) rest = rest.slice(1);
    let word: string;
    let prefix = false;
    if (rest.startsWith('"')) {
      const quoted = rest.slice(1);
      const end = quoted.indexOf('"');
      word = end < 0 ? quoted : quoted.slice(0, end);
      rest = end < 0 ? "" : quoted.slice(end + 1);
    } else {
      const end = rest.search(/\s/);
      const raw = end < 0 ? rest : rest.slice(0, end);
      word = raw.replace(/\*+$/, "");
      prefix = word.length < raw.length;
      rest = end < 0 ? "" : rest.slice(end);
    }
    rest = rest.trimStart();
    const words = tokens(word);
    if (words.length > 0) terms.push({ tokens: words, prefix, negated });
  }
  return terms;
}

/** The term's tokens appear in a row in `column` (one of program and message). */
function appears(term: Term, column: readonly string[]): boolean {
  const n = term.tokens.length;
  for (let start = 0; start + n <= column.length; start++) {
    let all = true;
    for (let i = 0; i < n && all; i++) {
      const want = term.tokens[i] ?? "";
      const have = column[start + i] ?? "";
      all = term.prefix && i === n - 1 ? have.startsWith(want) : have === want;
    }
    if (all) return true;
  }
  return false;
}

/** Whether a record's program and message match the terms. */
export function matchesText(record: LogRecord, terms: readonly Term[]): boolean {
  if (terms.length === 0) return true;
  const columns = [tokens(record.program ?? ""), tokens(record.message)];
  const found = (term: Term) => columns.some((column) => appears(term, column));
  return terms.every((term) => found(term) !== term.negated);
}

/** The filter as a test of one record, with the times of the page's window. */
export function admitter(
  filter: Filter,
  window: { from: number; to: number | null },
): (record: LogRecord) => boolean {
  const terms = filter.text === null ? [] : parseText(filter.text);
  const nodes = new Set(filter.nodes);
  const hosts = new Set(filter.hosts);
  const programs = new Set(filter.programs);
  const min = filter.min === null ? 7 : severityCode(filter.min);
  return (record) =>
    // Nodes and hosts are one set of sources.
    ((nodes.size === 0 && hosts.size === 0) ||
      (record.node !== null && nodes.has(record.node)) ||
      (record.host !== null && hosts.has(record.host))) &&
    (programs.size === 0 || (record.program !== null && programs.has(record.program))) &&
    severityCode(record.severity) <= min &&
    (!filter.annotated || record.pattern !== null) &&
    record.time >= window.from &&
    (window.to === null || record.time < window.to) &&
    matchesText(record, terms);
}

/**
 * Where the wanted terms' words appear in `text`, as `[start, end)` spans in order, for marking
 * them: each word whole, or its start for the last word of a `prefix*`. Case is ignored.
 */
export function highlights(text: string, terms: readonly Term[]): [number, number][] {
  const spans: [number, number][] = [];
  const words = text.toLowerCase();
  const wanted = terms.filter((term) => !term.negated);
  for (const match of words.matchAll(/[\p{L}\p{N}\p{Co}]+/gu)) {
    const word = match[0];
    const start = match.index;
    for (const term of wanted) {
      const length = term.tokens.reduce((found, token, i) => {
        if (found > 0) return found;
        if (term.prefix && i === term.tokens.length - 1 && word.startsWith(token)) {
          return token.length;
        }
        return word === token ? word.length : 0;
      }, 0);
      if (length > 0) {
        spans.push([start, start + length]);
        break;
      }
    }
  }
  return spans;
}
