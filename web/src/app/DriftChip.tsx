import * as Popover from "@kobalte/core/popover";
import { FilePen, FileX } from "lucide";
import { createMemo, createSignal, For, Show, useContext } from "solid-js";
import { unacknowledged } from "../lib/config";
import { formatDuration } from "../lib/format";
import { LiveContext } from "../lib/live";
import { SessionContext } from "../lib/session";
import { strings } from "../lib/strings";
import { Icon } from "../ui/Icon";

const t = strings.changes.drift;

/**
 * The top bar's drift (lifecycle.md, Drift; D-074), for admins, beside the pending changes: how
 * many managed packages changed on their devices and wait for a decision (not acknowledged), and
 * a popover of them, each opening its device's Config tab, with the review that overwrites them.
 */
export function DriftChip() {
  const session = useContext(SessionContext);
  return (
    <Show when={session.user.role === "admin"}>
      <Chip />
    </Show>
  );
}

function Chip() {
  const live = useContext(LiveContext);
  const [open, setOpen] = createSignal(false);
  const [now, setNow] = createSignal(Date.now());
  const drifts = createMemo(() => unacknowledged(live.config));
  const names = createMemo(() => new Map(live.fleet.nodes.map((n) => [n.id, n.name])));
  const count = () => drifts().length;

  return (
    <Show when={count() > 0}>
      <Popover.Root
        open={open()}
        onOpenChange={(next) => {
          setNow(Date.now());
          setOpen(next);
        }}
        placement="bottom-end"
        gutter={8}
      >
        <Popover.Trigger
          aria-label={t.label(count())}
          class="relative flex h-8 items-center gap-2 rounded-control border border-warn/40 bg-warn/10 px-2.5 font-medium text-fg text-sm transition-colors hover:bg-warn/20 data-expanded:bg-warn/20 max-sm:w-8 max-sm:justify-center max-sm:px-0"
        >
          <Icon icon={FilePen} class="size-4 shrink-0 text-warn" />
          <span class="hidden sm:inline">{t.text}</span>
          <span
            aria-hidden="true"
            class="h-4 min-w-4 rounded-full bg-warn px-1 text-center font-semibold text-[10px] text-canvas leading-4 tabular-nums max-sm:absolute max-sm:-top-1 max-sm:-right-1 max-sm:ring-2 max-sm:ring-canvas"
          >
            {count() > 99 ? "99+" : count()}
          </span>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content class="z-50 flex max-h-[min(30rem,75vh)] w-[min(22rem,calc(100vw-2rem))] flex-col rounded-card border border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
            <div class="border-edge border-b px-4 py-3">
              <Popover.Title class="font-semibold text-sm">{t.title}</Popover.Title>
              <Popover.Description class="mt-0.5 text-fg-subtle text-xs">
                {t.hint}
              </Popover.Description>
            </div>
            <ul class="min-h-0 overflow-y-auto p-1.5">
              <For each={drifts()}>
                {(drift) => (
                  <li>
                    <a
                      href={`/devices/${drift.node}?tab=config`}
                      onClick={() => setOpen(false)}
                      class="flex items-center gap-3 rounded-control px-2 py-2 hover:bg-raised"
                    >
                      <span class="grid size-8 shrink-0 place-items-center rounded-lg bg-warn/10 text-warn">
                        <Icon icon={drift.removed ? FileX : FilePen} class="size-4" />
                      </span>
                      <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span class="truncate text-sm">
                          <span class="font-medium text-fg">
                            {names().get(drift.node) ?? `#${drift.node}`}
                          </span>
                          <span class="text-fg-subtle"> · </span>
                          <span class="font-mono text-fg-muted text-xs">{drift.package}</span>
                        </span>
                        <span class="text-fg-subtle text-xs tabular-nums">
                          <Show when={drift.removed}>{t.removed} · </Show>
                          {t.since(formatDuration(Math.max(0, now() - drift.changed_at) / 1000))}
                        </span>
                      </span>
                    </a>
                  </li>
                )}
              </For>
            </ul>
            <div class="border-edge border-t px-4 py-2.5 text-sm">
              <a
                href="/changes"
                onClick={() => setOpen(false)}
                class="font-medium text-accent hover:underline"
              >
                {t.review}
              </a>
            </div>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </Show>
  );
}
