import type { Backup } from "../../../api/gen/Backup";
import type { Job } from "../../../api/gen/Job";
import { formatBytes, formatDateTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";

const b = strings.device.backups;

/** A backup or restore job that touches `node`: its own, or a weekly one that includes it. */
export function concerns(job: Job, node: number): boolean {
  if (job.kind !== "backup" && job.kind !== "restore") return false;
  return job.node === node || job.nodes.some((entry) => entry.node === node);
}

/**
 * How many of the jobs that touch `node` ended after being seen running (`seen` remembers them):
 * the list is read again as it grows, but not for the ended jobs the topic's snapshot brings.
 */
export function endedCount(jobs: readonly Job[], node: number, seen: Set<number>): number {
  let ended = 0;
  for (const job of jobs) {
    if (!concerns(job, node)) continue;
    if (job.state === "running") seen.add(job.id);
    else if (seen.has(job.id)) ended += 1;
  }
  return ended;
}

/** The second line of a backup: why it was taken, its size, and since when it is unchanged. */
export function backupDetail(backup: Backup): string {
  const parts = [b.reason[backup.reason], formatBytes(backup.size_bytes)];
  if (backup.seen_at > backup.taken_at) parts.push(b.unchanged(formatDateTime(backup.seen_at)));
  return parts.join(" · ");
}
