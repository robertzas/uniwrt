import { createEffect, createSignal, Show, useContext } from "solid-js";
import type { KickOutcome } from "../../api/gen/KickOutcome";
import { ApiError, api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Segmented } from "../../ui/Segmented";
import { showToast } from "../../ui/Toast";

const k = strings.client.kickDialog;
const b = strings.client.blockDialog;

/** How long a kick keeps the client off, in seconds (features/wifi.md, Client actions). */
type Ban = keyof typeof k.ban;
const BANS: readonly Ban[] = ["0", "60", "600", "3600"];

/** What a kick did, for a toast: where from, or why nowhere. */
export function kickToast(outcome: KickOutcome, ban: Ban, nodeName: (id: number) => string) {
  if (outcome.nodes === 0) {
    return { title: k.noDevices, description: k.noDevicesHint, tone: "warn" as const };
  }
  if (outcome.failed.length > 0) {
    const devices = outcome.failed.map((f) => nodeName(f.node)).join(", ");
    return {
      title: k.refused(devices),
      description: outcome.failed.map((f) => f.error).join("; "),
      tone: "crit" as const,
    };
  }
  const where = [...new Set(outcome.from.map((f) => nodeName(f.node)))].join(", ");
  return {
    title: where ? k.kicked(where) : k.notConnected,
    description: ban === "0" ? undefined : k.kept(k.ban[ban]),
    tone: "ok" as const,
  };
}

/** Kicks the client off every managed access point, for a while the admin picks (1 min first). */
export function KickDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mac: string;
}) {
  const live = useContext(LiveContext);
  const [ban, setBan] = createSignal<Ban>("60");
  const [busy, setBusy] = createSignal(false);
  createEffect(
    () => props.open,
    (open) => {
      if (open) setBan("60");
    },
  );
  const nodeName = (id: number) =>
    live.fleet.nodes.find((node) => node.id === id)?.name ?? `#${id}`;
  const kick = async (event: SubmitEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      const outcome = await api.clients.kick(props.mac, { ban_secs: Number(ban()) });
      showToast(kickToast(outcome, ban(), nodeName));
      props.onOpenChange(false);
    } catch (err) {
      showToast({
        title: k.failed,
        description: err instanceof ApiError ? err.message : undefined,
        tone: "crit",
      });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={k.title}
      description={k.description}
    >
      <form class="flex flex-col gap-4 px-5 pt-2 pb-5" onSubmit={kick}>
        <div class="flex flex-col gap-1.5">
          <span class="font-medium text-sm">{k.label}</span>
          <Segmented
            label={k.label}
            value={ban()}
            options={BANS.map((value) => ({ value, label: k.ban[value] }))}
            onChange={setBan}
            class="self-start"
          />
        </div>
        <div class="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={() => props.onOpenChange(false)}>
            {k.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {k.kick}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** Asks before blocking: a block lasts until an admin lifts it. */
export function BlockDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mac: string;
  name: string;
  /** A private address: a block may not follow the device. */
  randomized: boolean;
}) {
  const [busy, setBusy] = createSignal(false);
  const block = async () => {
    setBusy(true);
    try {
      await api.clients.block(props.mac);
      showToast({ title: b.blockedToast, tone: "ok" });
      props.onOpenChange(false);
    } catch (err) {
      showToast({
        title: b.failed,
        description: err instanceof ApiError ? err.message : undefined,
        tone: "crit",
      });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={b.title(props.name)}
      description={b.description}
    >
      <div class="flex flex-col gap-4 px-5 pt-2 pb-5">
        <Show when={props.randomized}>
          <p class="text-fg-muted text-sm">{b.note}</p>
        </Show>
        <div class="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={() => props.onOpenChange(false)}>
            {b.cancel}
          </Button>
          <Button type="button" variant="primary" disabled={busy()} onClick={block}>
            {b.block}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

/** Lifts the client's block at once (nothing to lose by it), saying how it went. */
export async function unblock(mac: string) {
  try {
    await api.clients.unblock(mac);
    showToast({ title: b.unblockedToast, tone: "ok" });
  } catch (err) {
    showToast({
      title: b.failed,
      description: err instanceof ApiError ? err.message : undefined,
      tone: "crit",
    });
  }
}
