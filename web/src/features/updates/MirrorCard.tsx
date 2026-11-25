import { RefreshCw } from "lucide";
import { createSignal, For, onCleanup, onSettled, Show } from "solid-js";
import type { FeedsView } from "../../api/gen/FeedsView";
import type { ImageStatus } from "../../api/gen/ImageStatus";
import type { MirrorSettings } from "../../api/gen/MirrorSettings";
import { api, errorText } from "../../lib/api";
import { formatBytes, formatDateTime, NONE } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { type Column, Table } from "../../ui/Table";
import { showToast } from "../../ui/Toast";
import { MirrorSet } from "./MirrorSet";
import { MIRROR_POLL_MS, orderedSets, setKey } from "./mirror";

const t = strings.firmware.mirror;

const imageColumns: Column<ImageStatus>[] = [
  {
    id: "name",
    header: t.images.name,
    class: "min-w-64",
    cell: (image) => (
      <span class="block truncate font-mono text-xs" title={image.url}>
        {image.name}
      </span>
    ),
  },
  {
    id: "size",
    header: t.images.size,
    align: "end",
    cell: (image) => (image.bytes === null ? NONE : formatBytes(image.bytes)),
  },
  {
    id: "state",
    header: t.images.state,
    class: "max-w-72",
    cell: (image) => (
      <Show
        when={image.error}
        fallback={
          <Badge tone={image.kept ? "ok" : "neutral"}>
            {image.kept ? t.images.kept : t.images.pending}
          </Badge>
        }
      >
        {(error) => (
          <span class="flex flex-col items-start gap-0.5 whitespace-normal">
            <Badge tone="crit">{t.error}</Badge>
            <span class="break-words text-fg-muted text-xs">{error()}</span>
          </span>
        )}
      </Show>
    ),
  },
];

/**
 * Offline packages (D-147): the controller's feed mirror. Admins turn it on or off and choose
 * *Keep whole feeds* (confirmed: ~3 GB a set) and start a refresh; everyone sees when it last
 * refreshed and why it failed, everything it keeps, each feed set (`MirrorSet`) and the
 * official images kept for upgrades on offer. While a sync runs the card reads the mirror
 * again every [`MIRROR_POLL_MS`].
 */
export function MirrorCard(props: { admin: boolean }) {
  const [view, setView] = createSignal<FeedsView>();
  const [error, setError] = createSignal<string>();
  const [saving, setSaving] = createSignal(false);
  const [starting, setStarting] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let controller: AbortController | undefined;

  const show = (found: FeedsView) => {
    setView(found);
    setError(undefined);
    clearTimeout(timer);
    if (found.syncing) timer = setTimeout(read, MIRROR_POLL_MS);
  };
  const read = () => {
    controller?.abort();
    controller = new AbortController();
    api.feeds
      .view(controller.signal)
      .then(show)
      .catch((err: unknown) => {
        if (!controller?.signal.aborted) setError(errorText(err));
      });
  };
  onSettled(() => {
    read();
    return () => controller?.abort();
  });
  onCleanup(() => clearTimeout(timer));

  const save = (settings: MirrorSettings) => {
    setSaving(true);
    api.feeds
      .settings(settings)
      .then((found) => {
        show(found);
        showToast({ title: t.saved, tone: "ok" });
      })
      .catch((err: unknown) =>
        showToast({ title: t.saveFailed, description: errorText(err), tone: "crit" }),
      )
      .finally(() => setSaving(false));
  };
  const refresh = () => {
    setStarting(true);
    api.feeds
      .refresh()
      .then((found) => {
        // The sync starts as the answer leaves: read until it ends.
        show({ ...found, syncing: true });
        showToast({ title: t.refreshStarted, tone: "ok" });
      })
      .catch((err: unknown) =>
        showToast({ title: t.refreshFailed, description: errorText(err), tone: "crit" }),
      )
      .finally(() => setStarting(false));
  };

  return (
    <Card class="mx-auto w-full max-w-7xl" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && view()?.settings.enabled}>
            <Button
              size="sm"
              icon={RefreshCw}
              disabled={starting() || view()?.syncing}
              onClick={refresh}
            >
              {t.refresh}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4 text-sm">
        <RequestError text={error()} />
        <Show
          when={view()}
          fallback={
            <Show when={!error()}>
              <p role="status" class="py-6 text-center text-fg-subtle">
                {t.loading}
              </p>
            </Show>
          }
        >
          {(found) => (
            <>
              <Show
                when={props.admin}
                fallback={
                  <p class="text-fg-muted">
                    {found().settings.enabled ? t.viewerOn(found().settings.whole_feeds) : t.off}
                  </p>
                }
              >
                <div class="flex flex-wrap items-start gap-x-8 gap-y-3">
                  <Checkbox
                    label={t.enabled}
                    hint={t.enabledHint}
                    checked={found().settings.enabled}
                    disabled={saving()}
                    onChange={(enabled) => save({ ...found().settings, enabled })}
                    class="max-w-md"
                  />
                  <Checkbox
                    label={t.whole}
                    hint={t.wholeHint}
                    checked={found().settings.whole_feeds || confirming()}
                    disabled={saving() || !found().settings.enabled}
                    onChange={(whole) =>
                      whole
                        ? setConfirming(true)
                        : save({ ...found().settings, whole_feeds: false })
                    }
                    class="max-w-md"
                  />
                </div>
              </Show>
              <Show when={props.admin && !found().settings.enabled}>
                <p class="text-fg-muted text-xs">{t.off}</p>
              </Show>
              <div class="flex flex-wrap items-center gap-x-4 gap-y-1 border-edge border-t pt-3 text-fg-muted text-xs">
                <Show
                  when={found().syncing}
                  fallback={
                    <span>
                      {found().synced_at === null
                        ? t.neverSynced
                        : t.synced(formatDateTime(found().synced_at ?? 0))}
                    </span>
                  }
                >
                  <span role="status" class="flex items-center gap-1.5">
                    <Icon icon={RefreshCw} class="size-3.5 shrink-0 motion-safe:animate-spin" />
                    {t.refreshing}
                  </span>
                </Show>
                <span class="font-medium text-fg tabular-nums">
                  {t.kept(formatBytes(found().bytes))}
                </span>
              </div>
              <Show when={found().sync_error}>
                {(why) => (
                  <p class="break-words rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-fg text-xs">
                    {why()}
                  </p>
                )}
              </Show>
              <Show
                when={found().sets.length > 0}
                fallback={<p class="text-fg-subtle text-xs">{t.noSets}</p>}
              >
                <div class="flex flex-col gap-2">
                  <For each={orderedSets(found().sets)} keyed={(status) => setKey(status.set)}>
                    {(status) => <MirrorSet status={status()} />}
                  </For>
                </div>
              </Show>
              <Show when={found().images.length > 0}>
                <section aria-label={t.images.title} class="flex flex-col gap-1.5">
                  <h3 class="font-medium text-fg text-sm">{t.images.title}</h3>
                  <Table
                    label={t.images.title}
                    columns={imageColumns}
                    rows={found().images}
                    rowKey={(image) => image.url}
                  />
                </section>
              </Show>
              <Dialog
                open={confirming()}
                onOpenChange={setConfirming}
                title={t.confirm.title}
                description={t.confirm.body(Math.max(found().sets.length, 1))}
              >
                <DialogFooter class="px-5 pb-5">
                  <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
                    {t.confirm.cancel}
                  </Button>
                  <Button
                    size="sm"
                    variant="primary"
                    onClick={() => {
                      setConfirming(false);
                      save({ ...found().settings, whole_feeds: true });
                    }}
                  >
                    {t.confirm.keep}
                  </Button>
                </DialogFooter>
              </Dialog>
            </>
          )}
        </Show>
      </div>
    </Card>
  );
}
