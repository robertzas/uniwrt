/**
 * A semantic UCI diff as the review screens show it (`uniwrt_uci::Diff`: an apply plan's
 * packages, an import's residual). Structural types, so `ui/` needs nothing from `api/`.
 */

/** A value: one string (`option`) or several (`list`). */
export type UciValue = string | string[];

/** Where a section is: as `uci` addresses an anonymous one, `@type[index]`. */
export interface UciSectionId {
  type: string;
  name?: string;
  index: number;
}

export interface UciOptionDiff {
  name: string;
  before: UciValue | null;
  after: UciValue | null;
}

export interface UciSectionDiff {
  before: UciSectionId | null;
  after: UciSectionId | null;
  moved: boolean;
  options: readonly UciOptionDiff[];
}

export interface UciDiff {
  package: string;
  sections: readonly UciSectionDiff[];
}

/** What a plan shows in place of a secret's value (`uniwrt_model::apply::MASKED`). */
export const MASKED = "••••••••";

export type SectionChange = "added" | "removed" | "changed" | "moved";

/** How a section changed: added, removed, its options changed, or only its place. */
export function sectionChange(diff: UciSectionDiff): SectionChange {
  if (!diff.before) return "added";
  if (!diff.after) return "removed";
  return diff.options.length > 0 ? "changed" : "moved";
}

/** A section as `uci` addresses it: by its name, else `@type[index]`. */
export function sectionLabel(id: UciSectionId): string {
  return id.name === undefined ? `@${id.type}[${id.index}]` : `'${id.name}'`;
}

/** The section it was, when it was renamed (or a named one replaced an anonymous one). */
export function renamedFrom(diff: UciSectionDiff): string | undefined {
  const { before, after } = diff;
  if (!before || !after || before.name === after.name) return undefined;
  return sectionLabel(before);
}

/** Sections added, removed and changed (moved counts as changed) in a package. */
export function diffCounts(diff: UciDiff): { added: number; removed: number; changed: number } {
  const out = { added: 0, removed: 0, changed: 0 };
  for (const section of diff.sections) {
    const change = sectionChange(section);
    out[change === "moved" ? "changed" : change] += 1;
  }
  return out;
}

/** One line of an option's change, as `uci` would write the option. */
export interface OptionLine {
  op: "+" | "-" | "~";
  keyword: "option" | "list";
  name: string;
  /** The value (the new one when changed). */
  value: string;
  /** The old value of a changed option. */
  before?: string;
}

const items = (value: UciValue): string[] => (Array.isArray(value) ? value : [value]);

/**
 * An option's change as lines: added and removed options whole, a changed option as old → new,
 * and a changed list item by item (a list that only reordered, as old → new).
 */
export function optionLines(option: UciOptionDiff): OptionLine[] {
  const { name, before, after } = option;
  const lines = (op: "+" | "-", value: UciValue): OptionLine[] =>
    Array.isArray(value)
      ? value.map((item) => ({ op, keyword: "list", name, value: item }))
      : [{ op, keyword: "option", name, value }];
  if (before === null) return after === null ? [] : lines("+", after);
  if (after === null) return lines("-", before);
  if (!Array.isArray(before) && !Array.isArray(after)) {
    return [{ op: "~", keyword: "option", name, value: after, before }];
  }
  if (Array.isArray(before) !== Array.isArray(after)) {
    return [...lines("-", before), ...lines("+", after)];
  }
  const old = items(before);
  const now = items(after);
  const removed = old.filter((item) => !now.includes(item));
  const added = now.filter((item) => !old.includes(item));
  if (removed.length === 0 && added.length === 0) {
    return [{ op: "~", keyword: "list", name, value: now.join(" "), before: old.join(" ") }];
  }
  return [
    ...removed.map((value) => ({ op: "-" as const, keyword: "list" as const, name, value })),
    ...added.map((value) => ({ op: "+" as const, keyword: "list" as const, name, value })),
  ];
}
