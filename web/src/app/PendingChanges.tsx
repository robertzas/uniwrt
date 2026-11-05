import { useLocation } from "@solidjs/router";
import { LoaderCircle, Upload } from "lucide";
import { createEffect, createMemo, Show, useContext } from "solid-js";
import type { JobId } from "../api/gen/JobId";
import { pendingNodes } from "../lib/config";
import { createJobsView } from "../lib/jobs";
import { LiveContext } from "../lib/live";
import { SessionContext } from "../lib/session";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";
import { showToast } from "../ui/Toast";

const t = strings.changes;

/**
 * The top bar's pending changes (ui.md, Interaction patterns; D-071), for admins: how many managed
 * devices the latest revision has yet to reach, or an apply running, linking to the review. An
 * apply that ends while another page is open says how it went in a toast.
 */
export function PendingChanges() {
  const session = useContext(SessionContext);
  return (
    <Show when={session.user.role === "admin"}>
      <Tray />
    </Show>
  );
}

function Tray() {
  const live = useContext(LiveContext);
  const location = useLocation();
  const [jobs] = createJobsView(live);
  const pending = createMemo(() => pendingNodes(live.config, live.fleet.nodes).length);
  const applies = createMemo(() => Object.values(jobs.byId).filter((job) => job.kind === "apply"));
  const applying = () => applies().some((job) => job.state === "running");

  /** The applies seen running, to tell when they end. */
  const watching = new Set<JobId>();
  createEffect(
    () =>
      applies().map((job) => ({
        id: job.id,
        state: job.state,
        revision: job.revision,
        error: job.error?.message,
      })),
    (list) => {
      for (const job of list) {
        if (job.state === "running") {
          watching.add(job.id);
        } else if (watching.delete(job.id) && location.pathname !== "/changes") {
          showToast(
            job.state === "succeeded"
              ? { title: t.progress.done.ok(job.revision ?? 0), tone: "ok" }
              : { title: t.progress.done.failed, description: job.error, tone: "crit" },
          );
        }
      }
    },
  );

  return (
    <Show when={applying() || pending() > 0}>
      <a
        href="/changes"
        aria-label={applying() ? t.tray.applyingLabel : t.tray.label(pending())}
        class="relative flex h-8 items-center gap-2 rounded-control border border-accent/40 bg-accent/10 px-2.5 font-medium text-fg text-sm transition-colors hover:bg-accent/20 max-sm:w-8 max-sm:justify-center max-sm:px-0"
      >
        <Icon
          icon={applying() ? LoaderCircle : Upload}
          class={["size-4 shrink-0 text-accent", { "animate-spin": applying() }]}
        />
        <span class="hidden sm:inline">{applying() ? t.tray.applying : t.tray.text}</span>
        <Show when={!applying()}>
          <span
            aria-hidden="true"
            class="h-4 min-w-4 rounded-full bg-accent-fill px-1 text-center font-semibold text-[10px] text-on-accent leading-4 tabular-nums max-sm:absolute max-sm:-top-1 max-sm:-right-1 max-sm:ring-2 max-sm:ring-canvas"
          >
            {pending() > 99 ? "99+" : pending()}
          </span>
        </Show>
      </a>
    </Show>
  );
}
