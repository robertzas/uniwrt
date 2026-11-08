import type { DnsRecord } from "../../api/gen/DnsRecord";
import { ipv4 } from "../../lib/ipv4";
import { isDnsName } from "../../lib/names";
import { strings } from "../../lib/strings";

const e = strings.networks.dns.errors;

/** A record as its row holds it: the address or the target as typed. */
export interface RecordDraft {
  kind: DnsRecord["kind"];
  name: string;
  value: string;
}

export type RecordErrors = { name?: string; value?: string };

export function recordDraft(record: DnsRecord): RecordDraft {
  return {
    kind: record.kind,
    name: record.name,
    value: record.kind === "host" ? record.ip : record.target,
  };
}

/** A rough IPv6 check; the server parses it fully. */
function isIpv6(text: string): boolean {
  return /^[0-9a-f:]+$/i.test(text) && text.includes(":") && text.split("::").length <= 2;
}

/** Whether `name` is the controller's own record (`uniwrt`, or `uniwrt.<domain>`). */
function isController(name: string, domain: string): boolean {
  const n = name.toLowerCase().replace(/\.$/, "");
  return n === "uniwrt" || n === `uniwrt.${domain.toLowerCase()}`;
}

/** The rows' records, or each row's errors, checked as the server checks them. */
export function recordsOf(
  drafts: readonly RecordDraft[],
  domain: string,
): { records?: DnsRecord[]; errors: RecordErrors[] } {
  const errors = drafts.map((draft): RecordErrors => {
    const name = draft.name.trim();
    const value = draft.value.trim();
    const found: RecordErrors = {};
    if (!isDnsName(name)) found.name = e.name;
    else if (isController(name, domain)) found.name = e.controller(domain);
    if (draft.kind === "host" && ipv4(value) === undefined && !isIpv6(value)) found.value = e.ip;
    if (draft.kind === "cname" && !isDnsName(value)) found.value = e.target;
    return found;
  });
  if (errors.some((found) => found.name || found.value)) return { errors };
  return {
    records: drafts.map((draft) =>
      draft.kind === "host"
        ? { kind: "host", name: draft.name.trim(), ip: draft.value.trim() }
        : { kind: "cname", name: draft.name.trim(), target: draft.value.trim() },
    ),
    errors,
  };
}

/** The server's issues on `services/dns_records/<i>/…` as the rows' errors. */
export function recordErrorsOfIssues(
  issues: readonly { path: string; message: string }[],
  count: number,
): { errors: RecordErrors[]; other: string[] } {
  const errors: RecordErrors[] = Array.from({ length: count }, () => ({}));
  const other: string[] = [];
  for (const issue of issues) {
    const match = issue.path.match(/^services\/dns_records\/(\d+)\/(name|ip|target)$/);
    const row = match ? errors[Number(match[1])] : undefined;
    if (row && match) {
      const field = match[2] === "name" ? "name" : "value";
      row[field] ??= issue.message;
    } else other.push(issue.message);
  }
  return { errors, other };
}
