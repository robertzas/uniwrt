import { LoaderCircle, TriangleAlert } from "lucide";
import { createEffect, createSignal, Match, onCleanup, Show, Switch } from "solid-js";
import type { BackupManifest } from "../../../api/gen/BackupManifest";
import type { RestoreState } from "../../../api/gen/RestoreState";
import { api, errorText } from "../../../lib/api";
import { followRestore } from "../../../lib/controller";
import { formatBytes, formatDateTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { FormField } from "../../../ui/FormField";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import { showToast } from "../../../ui/Toast";

const t = strings.settings.backups.restore;

export interface ControllerRestoreProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** On the first-run screen of a new host (no session; nothing to replace). */
  firstRun?: boolean;
  /** Called when the staged restore changed (uploaded, unlocked or discarded). */
  onChanged?: () => void;
}

/**
 * Restores a controller backup (lifecycle.md, Controller restore; D-129): the file is uploaded,
 * opened with its passphrase (checked whole by the server), and what it holds is shown before
 * *Restore and restart*. The page then follows the restart and reloads once the controller is
 * back. A staged restore survives the dialog closing; it opens at the step it reached.
 */
export function ControllerRestore(props: ControllerRestoreProps) {
  const [state, setState] = createSignal<RestoreState>();
  const [file, setFile] = createSignal<File>();
  const [passphrase, setPassphrase] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [restarting, setRestarting] = createSignal(false);
  const [lost, setLost] = createSignal(false);
  let stop: (() => void) | undefined;
  onCleanup(() => stop?.());

  createEffect(
    () => props.open,
    (open) => {
      if (!open) return;
      setError(undefined);
      setPassphrase("");
      setFile(undefined);
      const controller = new AbortController();
      api.controller.restore
        .state(controller.signal)
        .then(setState)
        .catch((err) => {
          if (!controller.signal.aborted) setError(errorText(err));
        });
      return () => controller.abort();
    },
  );

  const run = async (step: () => Promise<void>) => {
    setBusy(true);
    setError(undefined);
    try {
      await step();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  const changed = (next: RestoreState) => {
    setState(next);
    props.onChanged?.();
  };
  const upload = () =>
    run(async () => {
      const chosen = file();
      if (chosen) changed(await api.controller.restore.upload(chosen));
    });
  const unlock = (event: SubmitEvent) => {
    event.preventDefault();
    void run(async () => {
      changed(await api.controller.restore.unlock({ passphrase: passphrase() }));
      setPassphrase("");
    });
  };
  const discard = () =>
    run(async () => {
      await api.controller.restore.discard();
      changed({ state: "none" });
      setFile(undefined);
      showToast({ title: t.discarded, tone: "ok" });
    });
  const apply = () =>
    run(async () => {
      await api.controller.restore.apply();
      setRestarting(true);
      stop = followRestore({
        health: (signal) => api.health(signal),
        state: (signal) => api.controller.restore.state(signal, false),
        onBack: () => window.location.reload(),
        onLost: () => setLost(true),
      });
    });

  const current = () => state()?.state;
  const uploadedSize = (): number | undefined => {
    const found = state();
    return found?.state === "uploaded" ? found.size_bytes : undefined;
  };
  const manifest = (): BackupManifest | undefined => {
    const found = state();
    return found?.state === "ready" ? found.manifest : undefined;
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={(open) => {
        if (!restarting()) props.onOpenChange(open);
      }}
      title={props.firstRun ? t.firstRunTitle : t.dialogTitle}
    >
      <div class="flex flex-col gap-4 px-5 pb-5">
        <RequestError text={error()} />
        <Switch>
          <Match when={restarting()}>
            <Show
              when={!lost()}
              fallback={
                <div class="flex flex-col gap-3">
                  <p role="alert" class="text-fg text-sm">
                    {t.lost}
                  </p>
                  <div class="flex justify-end">
                    <Button onClick={() => window.location.reload()}>{t.reload}</Button>
                  </div>
                </div>
              }
            >
              <div role="status" class="flex items-start gap-3 py-2">
                <Icon icon={LoaderCircle} class="mt-0.5 size-5 shrink-0 animate-spin text-accent" />
                <div class="text-sm">
                  <p class="font-medium">{t.restarting}</p>
                  <p class="text-fg-muted">{t.restartingHint}</p>
                </div>
              </div>
            </Show>
          </Match>
          <Match when={current() === "none"}>
            <FormField label={t.file} for="controller-backup-file" hint={t.choose}>
              <input
                id="controller-backup-file"
                type="file"
                accept=".age"
                disabled={busy()}
                class="text-sm file:mr-3 file:h-9 file:cursor-pointer file:rounded-control file:border file:border-edge file:bg-surface file:px-3 file:font-medium file:text-fg file:text-sm hover:file:bg-raised"
                onChange={(event) => setFile(event.currentTarget.files?.[0])}
              />
            </FormField>
            <Show when={busy() && file()}>
              {(chosen) => (
                <p role="status" class="flex items-center gap-2 text-fg-muted text-sm">
                  <Icon icon={LoaderCircle} class="size-4 animate-spin" />
                  {t.uploading(formatBytes(chosen().size))}
                </p>
              )}
            </Show>
            <div class="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => props.onOpenChange(false)}>
                {t.cancel}
              </Button>
              <Button variant="primary" disabled={!file() || busy()} onClick={upload}>
                {t.upload}
              </Button>
            </div>
          </Match>
          <Match when={uploadedSize() !== undefined}>
            <form class="flex flex-col gap-4" onSubmit={unlock}>
              <p class="text-fg-muted text-sm">{t.uploaded(formatBytes(uploadedSize()))}</p>
              <TextField
                label={t.passphrase}
                name="backup-passphrase"
                type="password"
                autocomplete="off"
                value={passphrase()}
                onInput={setPassphrase}
                required
                autofocus
              />
              <Show when={busy()}>
                <p role="status" class="flex items-center gap-2 text-fg-muted text-sm">
                  <Icon icon={LoaderCircle} class="size-4 animate-spin" />
                  {t.unlocking}
                </p>
              </Show>
              <div class="flex justify-end gap-2">
                <Button variant="ghost" disabled={busy()} onClick={discard}>
                  {t.discard}
                </Button>
                <Button type="submit" variant="primary" disabled={!passphrase() || busy()}>
                  {t.unlock}
                </Button>
              </div>
            </form>
          </Match>
          <Match when={manifest()}>
            {(found) => (
              <>
                <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 rounded-control border border-edge px-3 py-2.5 text-sm">
                  <dt class="text-fg-muted">{t.takenAt}</dt>
                  <dd class="tabular-nums">{formatDateTime(found().taken_at)}</dd>
                  <dt class="text-fg-muted">{t.version}</dt>
                  <dd class="tabular-nums">{found().version}</dd>
                  <dt class="text-fg-muted">{t.schema}</dt>
                  <dd class="tabular-nums">{found().schema}</dd>
                </dl>
                <p class="flex items-start gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-fg text-sm">
                  <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
                  <span>{props.firstRun ? t.firstRun : `${t.replaces} ${t.signInAfter}`}</span>
                </p>
                <div class="flex justify-end gap-2">
                  <Button variant="ghost" disabled={busy()} onClick={discard}>
                    {t.discard}
                  </Button>
                  <Button variant="primary" disabled={busy()} onClick={apply}>
                    {t.apply}
                  </Button>
                </div>
              </>
            )}
          </Match>
        </Switch>
      </div>
    </Dialog>
  );
}

/** The first-run screen's way in (D-129): a link under the setup form that opens the restore. */
export function SetupRestore() {
  const [open, setOpen] = createSignal(false);
  return (
    <>
      <button
        type="button"
        class="mt-4 w-full text-center text-accent text-sm hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
        onClick={() => setOpen(true)}
      >
        {t.firstRunLink}
      </button>
      <ControllerRestore open={open()} onOpenChange={setOpen} firstRun />
    </>
  );
}
