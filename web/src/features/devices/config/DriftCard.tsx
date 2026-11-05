import { useNavigate } from "@solidjs/router";
import {
  BellOff,
  Check,
  CircleCheck,
  Download,
  Eye,
  FilePen,
  FileX,
  TriangleAlert,
  Undo2,
} from "lucide";
import { createMemo, createSignal, For, onSettled, Show, useContext } from "solid-js";
import type { Drift } from "../../../api/gen/Drift";
import type { DriftMute } from "../../../api/gen/DriftMute";
import type { Node } from "../../../api/gen/Node";
import { api, errorText } from "../../../lib/api";
import { type ConfigState, driftsOf, mutesOf } from "../../../lib/config";
import { formatDuration } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { PackageDiff } from "../../../ui/PackageDiff";
import { showToast } from "../../../ui/Toast";

const d = strings.device.drift;

/** How long ago `at` was, at `now`. */
const ago = (at: number, now: number) => formatDuration(Math.max(0, now - at) / 1000);

/** Whether a managed node has drift or mutes to show: the card then comes first. */
export function hasDrift(config: ConfigState, node: Node): boolean {
  return (
    node.mode === "managed" &&
    (driftsOf(config, node.id).length > 0 || mutesOf(config, node.id).length > 0)
  );
}

/**
 * A managed node's drift (lifecycle.md, Drift; D-074), live on the `config` topic: each drifted
 * package's diff from what uniwrt confirmed, and for admins import it (the scoped import page),
 * overwrite it (the review), acknowledge it, or mute the package; muted packages watch again.
 */
export function DriftCard(props: { node: Node; admin: boolean }) {
  const live = useContext(LiveContext);
  const [now, setNow] = createSignal(Date.now());
  onSettled(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  });
  const drifts = createMemo(() => driftsOf(live.config, props.node.id));
  const mutes = createMemo(() => mutesOf(live.config, props.node.id));
  const confirmed = () => live.config.applied[props.node.id] !== undefined;

  return (
    <Card aria-label={d.title}>
      <CardHeader
        title={d.title}
        description={d.description}
        actions={
          <Show when={drifts().length > 0}>
            <Badge tone="warn" icon={TriangleAlert}>
              {d.count(drifts().length)}
            </Badge>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={drifts().length > 0}
          fallback={
            <p class="flex items-center gap-2 text-fg-muted text-sm" role="status">
              <Icon
                icon={confirmed() ? CircleCheck : Eye}
                class={["size-4 shrink-0", confirmed() ? "text-ok" : "text-fg-subtle"]}
              />
              {confirmed() ? d.none : d.waiting}
            </p>
          }
        >
          <For each={drifts()}>
            {(drift) => <DriftItem drift={drift} admin={props.admin} now={now()} />}
          </For>
        </Show>
        <Show when={mutes().length > 0}>
          <div class="flex flex-col gap-1.5 border-edge/60 border-t pt-3">
            <p class="flex items-center gap-1.5 font-medium text-fg-muted text-xs">
              <Icon icon={BellOff} class="size-3.5" />
              {d.muted}
            </p>
            <ul class="flex flex-col gap-1">
              <For each={mutes()}>
                {(mute) => <MuteItem mute={mute} admin={props.admin} now={now()} />}
              </For>
            </ul>
          </div>
        </Show>
      </div>
    </Card>
  );
}

/** Runs a drift request, saying how it went. */
async function act(run: () => Promise<void>, done: string, setBusy: (busy: boolean) => void) {
  setBusy(true);
  try {
    await run();
    showToast({ title: done, tone: "ok" });
  } catch (err) {
    showToast({ title: d.failed, description: errorText(err), tone: "crit" });
  } finally {
    setBusy(false);
  }
}

function DriftItem(props: { drift: Drift; admin: boolean; now: number }) {
  const navigate = useNavigate();
  const [busy, setBusy] = createSignal(false);
  const x = () => props.drift;
  const scope = () => `node=${x().node}&package=${encodeURIComponent(x().package)}`;
  return (
    <section
      aria-label={d.label(x().package)}
      class="flex flex-col gap-2.5 rounded-control border border-edge p-3"
    >
      <div class="flex flex-wrap items-center gap-2">
        <Icon
          icon={x().removed ? FileX : FilePen}
          class={["size-4 shrink-0", x().acknowledged ? "text-fg-subtle" : "text-warn"]}
        />
        <span class="font-medium font-mono text-fg text-sm">{x().package}</span>
        <Show
          when={x().acknowledged}
          fallback={<Badge tone="warn">{x().removed ? d.removed : d.drifted}</Badge>}
        >
          <Badge icon={Check}>{d.acknowledged}</Badge>
        </Show>
        <span class="text-fg-subtle text-xs">
          {d.changed(ago(x().changed_at, props.now), x().revision)}
        </span>
      </div>
      <Show when={x().acknowledged}>
        {(ack) => (
          <p class="text-fg-muted text-xs">
            {d.acknowledgedBy(ack().by, ago(ack().at, props.now))}
          </p>
        )}
      </Show>
      <Show when={x().removed}>
        <p class="text-fg-muted text-sm">{d.removedBody}</p>
      </Show>
      <Show when={x().error}>
        {(error) => <p class="text-crit text-sm">{d.unreadable(error())}</p>}
      </Show>
      <Show when={x().diff.sections.length > 0}>
        <PackageDiff diff={x().diff} />
      </Show>
      <Show when={props.admin}>
        <div class="flex flex-wrap justify-end gap-2">
          <Button
            variant="ghost"
            size="sm"
            icon={BellOff}
            title={d.muteHint}
            disabled={busy()}
            onClick={() =>
              act(
                () => api.drift.mute(x().node, x().package, true),
                d.done.muted(x().package),
                setBusy,
              )
            }
          >
            {d.mute}
          </Button>
          <Show when={!x().acknowledged}>
            <Button
              variant="ghost"
              size="sm"
              icon={Check}
              title={d.acknowledgeHint}
              disabled={busy()}
              onClick={() =>
                act(
                  () => api.drift.acknowledge(x().node, x().package),
                  d.done.acknowledged(x().package),
                  setBusy,
                )
              }
            >
              {d.acknowledge}
            </Button>
          </Show>
          <Button
            size="sm"
            icon={Undo2}
            title={d.overwriteHint}
            disabled={busy()}
            onClick={() => navigate("/changes")}
          >
            {d.overwrite}
          </Button>
          <Button
            variant="primary"
            size="sm"
            icon={Download}
            title={d.importHint}
            disabled={busy() || x().error !== undefined}
            onClick={() => navigate(`/settings/import?${scope()}`)}
          >
            {d.import}
          </Button>
        </div>
      </Show>
    </section>
  );
}

function MuteItem(props: { mute: DriftMute; admin: boolean; now: number }) {
  const [busy, setBusy] = createSignal(false);
  const m = () => props.mute;
  return (
    <li class="flex items-center justify-between gap-3 text-sm">
      <span class="min-w-0 truncate text-fg-muted">
        {d.mutedBy(m().package, m().by, ago(m().at, props.now))}
      </span>
      <Show when={props.admin}>
        <Button
          variant="ghost"
          size="sm"
          icon={Eye}
          disabled={busy()}
          aria-label={`${d.watch}: ${m().package}`}
          onClick={() =>
            act(
              () => api.drift.mute(m().node, m().package, false),
              d.done.watched(m().package),
              setBusy,
            )
          }
        >
          {d.watch}
        </Button>
      </Show>
    </li>
  );
}
