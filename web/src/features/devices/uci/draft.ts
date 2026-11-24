import type { UciTreeSection } from "../../../api/gen/UciTreeSection";
import type { Value } from "../../../api/gen/Value";
import { strings } from "../../../lib/strings";
import type { SectionDelta } from "./edits";
import { isMaskedOption, isUciName, sameValue } from "./uci";

const e = strings.device.uci.errors;

/** An option as the editor holds it. */
export interface OptionDraft {
  /** Its name as read; `undefined` for one added in the editor. */
  read?: string;
  name: string;
  list: boolean;
  /** One value, or a list's items one per line; a masked option's is empty until typed anew. */
  text: string;
  /** Masked: sealed in the vault once typed, kept as it is while empty. */
  secret: boolean;
  deleted: boolean;
}

/** The section's options as the editor starts them. */
export function draftOf(section: UciTreeSection): OptionDraft[] {
  return section.options.map((option) => ({
    read: option.name,
    name: option.name,
    list: Array.isArray(option.value),
    text: option.secret ? "" : [option.value].flat().join("\n"),
    secret: option.secret,
    deleted: false,
  }));
}

/** A new, empty option; its name decides later whether it is masked. */
export function newOption(): OptionDraft {
  return { name: "", list: false, text: "", secret: false, deleted: false };
}

/** The value typed: one string, or a list's non-empty lines. */
function typed(draft: OptionDraft): Value {
  return draft.list ? draft.text.split("\n").filter((line) => line !== "") : draft.text;
}

/**
 * What the drafts change in `section` (`undefined` for a new one), or what is wrong with each
 * option by its place. A masked option typed anew is sealed; one left empty keeps its value. A
 * new option whose name says it holds a secret is sealed too, so it is one value.
 */
export function deltaOf(
  section: UciTreeSection | undefined,
  drafts: readonly OptionDraft[],
): { delta: SectionDelta; errors: Record<number, string> } {
  const delta: SectionDelta = { set: {}, secrets: {}, unset: [] };
  const errors: Record<number, string> = {};
  const names = new Set<string>();
  drafts.forEach((draft, i) => {
    const read = section?.options.find((o) => o.name === draft.read);
    if (draft.deleted) {
      if (read) delta.unset.push(read.name);
      return;
    }
    const name = read ? read.name : draft.name.trim();
    if (!isUciName(name)) {
      errors[i] = name ? e.optionName : e.optionNameMissing;
      return;
    }
    if (names.has(name)) {
      errors[i] = e.optionTwice(name);
      return;
    }
    names.add(name);
    if (read ? read.secret : isMaskedOption(name)) {
      if (draft.text === "") {
        if (!read) errors[i] = e.secretMissing;
      } else if (draft.list) {
        errors[i] = e.secretList;
      } else if (/[\0\r\n]/.test(draft.text)) {
        errors[i] = e.lineBreak;
      } else {
        delta.secrets[name] = draft.text;
      }
      return;
    }
    const value = typed(draft);
    const items = [value].flat();
    if (items.some((item) => /[\0\r\n]/.test(item))) {
      errors[i] = e.lineBreak;
      return;
    }
    if (draft.list && items.length === 0) {
      errors[i] = e.emptyList;
      return;
    }
    if (!read || !sameValue(read.value, value)) delta.set[name] = value;
  });
  return { delta, errors };
}

/** Whether a delta changes nothing. */
export function isEmpty(delta: SectionDelta): boolean {
  return (
    Object.keys(delta.set).length === 0 &&
    Object.keys(delta.secrets).length === 0 &&
    delta.unset.length === 0
  );
}
