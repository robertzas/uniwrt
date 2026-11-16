import { Download, ExternalLink, RefreshCw, TriangleAlert } from "lucide";
import { createSignal, For, Match, onSettled, Show, Switch } from "solid-js";
import type { ControllerUpdate } from "../../api/gen/ControllerUpdate";
import type { ControllerUpdates } from "../../api/gen/ControllerUpdates";
import { api, errorText } from "../../lib/api";
import { followInstall, type InstallOutcome } from "../../lib/controller";
import { clockText, formatBytes, formatDateTime, nightlyTimes } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { Icon } from "../../ui/Icon";
import { JobSuccessNote, RequestError } from "../../ui/JobOutcome";
import { showToast } from "../../ui/Toast";

const t = strings.firmware.controller;

/**
 * The controller's own update (D-123): the running release, the newest one on offer with its
 * notes, Install for admins (followed through the restart: running, rolled back, or refused),
 * the last release that failed its trial, and the nightly automatic install. A controller that
 * can't install itself (a development build, demo mode, self-upgrade off) says why. Hidden
 * when the controller has no updates (the endpoint answers an error).
 */
export function ControllerCard(props: { admin: boolean }) {
  const [update, setUpdate] = createSignal<ControllerUpdate>();
  const [error, setError] = createSignal<string>();
  const [starting, setStarting] = createSignal(false);
  /** The version this page is installing, followed until an outcome. */
  const [following, setFollowing] = createSignal<string>();
  const [outcome, setOutcome] = createSignal<InstallOutcome>();
  const [saving, setSaving] = createSignal(false);

  onSettled(() => {
    const controller = new AbortController();
    api.controller
      .update(controller.signal)
      .then((found) => {
        setUpdate(found);
        // An install under way (started elsewhere, or before a reload) is followed too.
        const progress = found.progress;
        if (progress.state !== "idle" && progress.state !== "failed") {
          setFollowing(progress.version);
        }
      })
      .catch(() => {});
    return () => controller.abort();
  });

  followInstall({
    version: following,
    fetch: (signal) => api.controller.update(signal),
    onUpdate: setUpdate,
    onOutcome: (found) => {
      setFollowing(undefined);
      setOutcome(found);
    },
  });

  const install = async (version: string) => {
    setStarting(true);
    setError(undefined);
    setOutcome(undefined);
    try {
      setUpdate(await api.controller.install({ version }));
      setFollowing(version);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setStarting(false);
    }
  };

  const save = (settings: ControllerUpdates) => {
    setSaving(true);
    api.controller
      .settings(settings)
      .then((saved) => {
        setUpdate((found) => (found ? { ...found, settings: saved } : found));
        showToast({ title: t.saved, tone: "ok" });
      })
      .catch((err: unknown) =>
        showToast({ title: t.saveFailed, description: errorText(err), tone: "crit" }),
      )
      .finally(() => setSaving(false));
  };

  const progressText = (found: ControllerUpdate): string | undefined => {
    const progress = found.progress;
    switch (progress.state) {
      case "downloading":
        return t.progress.downloading(progress.version);
      case "checking":
        return t.progress.checking(progress.version);
      case "restarting":
        return t.progress.restarting(progress.version);
      default:
        return undefined;
    }
  };
  const failedText = (found: ControllerUpdate): string | undefined => {
    const progress = found.progress;
    return progress.state === "failed" ? t.failed(progress.version, progress.reason) : undefined;
  };
  const busy = (found: ControllerUpdate) =>
    following() !== undefined || progressText(found) !== undefined;

  return (
    <Show when={update()}>
      {(found) => (
        <Card class="mx-auto w-full max-w-7xl" aria-label={t.title}>
          <CardHeader title={t.title} description={t.description} />
          <div class="flex flex-col gap-3 px-4 pb-4 text-sm">
            <p class="font-medium tabular-nums">{t.running(found().current)}</p>
            <Show
              when={found().offer}
              fallback={
                <Show when={!found().note && !outcome()}>
                  <p class="text-fg-muted">{t.upToDate}</p>
                </Show>
              }
            >
              {(offer) => (
                <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span class="font-medium">{t.offer(offer().version)}</span>
                  <Show when={offer().published_at}>
                    {(at) => (
                      <span class="text-fg-subtle text-xs">
                        {t.published(formatDateTime(at()))}
                      </span>
                    )}
                  </Show>
                  <span class="text-fg-subtle text-xs">{formatBytes(offer().size_bytes)}</span>
                  <a
                    href={offer().notes}
                    target="_blank"
                    rel="noopener noreferrer"
                    aria-label={`${strings.firmware.notes} ${offer().version}`}
                    class="inline-flex items-center gap-1 text-accent text-xs hover:underline"
                  >
                    {strings.firmware.notes}
                    <Icon icon={ExternalLink} class="size-3" />
                  </a>
                  <Show when={props.admin && !found().blocked}>
                    <Button
                      size="sm"
                      variant="primary"
                      icon={Download}
                      class="ml-auto"
                      disabled={starting() || busy(found())}
                      onClick={() => install(offer().version)}
                    >
                      {starting() ? t.installing : t.install}
                    </Button>
                  </Show>
                </div>
              )}
            </Show>
            <Show when={found().blocked}>
              {(why) => <p class="text-fg-muted text-xs">{why()}</p>}
            </Show>
            <Show when={progressText(found())}>
              {(text) => (
                <p role="status" class="flex items-center gap-2 text-fg-muted">
                  <Icon icon={RefreshCw} class="size-4 shrink-0 motion-safe:animate-spin" />
                  {text()}
                </p>
              )}
            </Show>
            <Switch>
              <Match when={outcome()?.kind === "running" && outcome()}>
                {(done) => (
                  <div class="flex flex-wrap items-center gap-3">
                    <JobSuccessNote text={t.nowRunning(done().version)} />
                    <Button size="sm" icon={RefreshCw} onClick={() => window.location.reload()}>
                      {t.reload}
                    </Button>
                  </div>
                )}
              </Match>
              <Match when={outcome()?.kind === "lost" && outcome()}>
                {(lost) => <RequestError text={t.lost(lost().version)} />}
              </Match>
            </Switch>
            <RequestError text={failedText(found())} />
            <Show when={found().failure}>
              {(failure) => (
                <p role="alert" class="flex items-start gap-2 text-warn">
                  <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0" />
                  {t.rolledBack(failure().version, formatDateTime(failure().at), failure().reason)}
                </p>
              )}
            </Show>
            <Show when={found().note}>{(note) => <RequestError text={note()} />}</Show>
            <RequestError text={error()} />
            <Show
              when={props.admin}
              fallback={
                <p class="text-fg-muted text-xs">
                  {found().settings.auto ? t.autoAt(clockText(found().settings.at_min)) : t.autoOff}
                </p>
              }
            >
              <div class="flex flex-wrap items-start gap-3 border-edge border-t pt-3">
                <Checkbox
                  label={t.auto}
                  hint={t.autoHint}
                  checked={found().settings.auto}
                  disabled={saving()}
                  onChange={(auto) => save({ ...found().settings, auto })}
                />
                <Show when={found().settings.auto}>
                  <label class="flex items-center gap-1.5 text-xs">
                    <span class="text-fg-muted">{t.time}</span>
                    <select
                      class="h-7 rounded-control border border-edge bg-surface px-1.5 text-fg text-xs tabular-nums"
                      value={String(found().settings.at_min)}
                      disabled={saving()}
                      onChange={(event) =>
                        save({ ...found().settings, at_min: Number(event.currentTarget.value) })
                      }
                    >
                      <For each={nightlyTimes(found().settings.at_min)}>
                        {(minutes) => <option value={String(minutes)}>{clockText(minutes)}</option>}
                      </For>
                    </select>
                  </label>
                </Show>
              </div>
            </Show>
          </div>
        </Card>
      )}
    </Show>
  );
}
