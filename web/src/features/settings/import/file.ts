import type { Issue } from "../../../api/gen/Issue";
import type { SecretImport } from "../../../api/gen/SecretImport";
import { strings } from "../../../lib/strings";

const t = strings.settings.fileImport;

/** What the page reads of a picked file before the server previews it. */
export interface PickedFile {
  /** The file's name on disk. */
  name: string;
  /** The parsed document, sent as is. */
  value: unknown;
  revision: number;
  uniwrt: string;
  exportedAt: number;
  /** Whether it holds sealed secrets (then a passphrase opens them). */
  secrets: boolean;
}

/**
 * Reads a picked file's text as a site file (`{"format": "uniwrt-site", …}`): what the page
 * shows of it, or why it isn't one. The server checks the rest (version, schema, the document).
 */
export function readSiteFile(name: string, text: string): PickedFile | { error: string } {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return { error: t.notJson };
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { error: t.notSite };
  }
  const file = value as Record<string, unknown>;
  if (file.format !== "uniwrt-site") return { error: t.notSite };
  return {
    name,
    value,
    revision: typeof file.revision === "number" ? file.revision : 0,
    uniwrt: typeof file.uniwrt === "string" ? file.uniwrt : "?",
    exportedAt: typeof file.exported_at === "number" ? file.exported_at : 0,
    secrets: typeof file.secrets === "string" && file.secrets !== "",
  };
}

/** The paths of the secrets the file left missing. */
export function missingPaths(secrets: readonly SecretImport[]): Set<string> {
  return new Set(secrets.filter((s) => s.source === "missing").map((s) => s.path));
}

/**
 * The values given for missing secrets, by path, leaving out empty fields (a secret left empty
 * is saved without a value).
 */
export function filledSecrets(
  secrets: readonly SecretImport[],
  values: Readonly<Record<string, string>>,
): Record<string, string> {
  const filled: Record<string, string> = {};
  for (const path of missingPaths(secrets)) {
    const value = values[path];
    if (value) filled[path] = value;
  }
  return filled;
}

/**
 * The preview's errors and warnings, and which errors still block the accept: an error on a
 * missing secret that has a value now is the accept's to check (it answers 422 if the value
 * doesn't pass).
 */
export function sortFileIssues(
  issues: readonly Issue[],
  secrets: readonly SecretImport[],
  values: Readonly<Record<string, string>>,
): { errors: Issue[]; warnings: Issue[]; blocking: Issue[]; bySecret: Map<string, Issue> } {
  const missing = missingPaths(secrets);
  const errors = issues.filter((i) => i.severity === "error");
  const bySecret = new Map<string, Issue>();
  for (const issue of errors) {
    if (missing.has(issue.path) && !bySecret.has(issue.path)) bySecret.set(issue.path, issue);
  }
  return {
    errors,
    warnings: issues.filter((i) => i.severity !== "error"),
    blocking: errors.filter((i) => !(missing.has(i.path) && values[i.path])),
    bySecret,
  };
}
