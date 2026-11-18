import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { Archive, ArchiveRestore, CircleAlert, CircleCheck, Download, TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import { api, errorText } from "../../../lib/api";
import { clockText, formatBytes, formatDateTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { showToast } from "../../../ui/Toast";
import { ControllerRestore } from "./ControllerRestore";
import { PassphraseCard } from "./PassphraseCard";

const t = strings.settings.backups;

/** How often the list is read while a backup is being taken elsewhere (the nightly one). */
const RUNNING_POLL_MS = 3000;

/**
 * Settings → Backups (lifecycle.md, Controller; D-129): the controller's own backups, newest first,
 * how the last went and where they are kept; for admins *Back up now*, the downloads, the
 * passphrase and the restore from a file.
 */
export function BackupsTab(props: { admin: boolean }) {
  const status = useQuery(() => ({
    queryKey: ["controller-backups"],
    queryFn: ({ signal }) => api.controller.backups.status(signal),
    placeholderData: keepPreviousData,
    refetchInterval: (query) => (query.state.data?.running ? RUNNING_POLL_MS : false),
  }));
  const [taking, setTaking] = createSignal(false);
  const [restoring, setRestoring] = createSignal(false);

  const backUp = async () => {
    setTaking(true);
    try {
      await api.controller.backups.create();
      showToast({ title: t.backedUp, tone: "ok" });
    } catch (err) {
      showToast({ title: t.backUpFailed, description: errorText(err), tone: "crit" });
    } finally {
      setTaking(false);
      void status.refetch();
    }
  };

  return (
    <div class="flex flex-col gap-4">
      <RequestError text={status.isError ? errorText(status.error) : undefined} />
      <Show when={status.data} fallback={<p class="text-fg-subtle text-sm">{t.loading}</p>}>
        {(found) => (
          <>
            <Card aria-label={t.title}>
              <CardHeader
                title={t.title}
                description={t.hint(clockText(found().at_min))}
                actions={
                  <Show when={props.admin}>
                    <Button
                      size="sm"
                      variant="secondary"
                      icon={Archive}
                      disabled={!found().passphrase_set || found().running || taking()}
                      onClick={backUp}
                    >
                      {taking() || found().running ? t.backingUp : t.backUp}
                    </Button>
                  </Show>
                }
              />
              <div class="flex flex-col gap-3 px-4 pb-4">
                <Show when={!found().passphrase_set}>
                  <p
                    role="alert"
                    class="flex items-start gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-fg text-sm"
                  >
                    <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
                    {props.admin ? t.noPassphrase : t.noPassphraseViewer}
                  </p>
                </Show>
                <Show when={found().last}>
                  {(last) => (
                    <Show
                      when={last().error}
                      fallback={
                        <p class="flex items-center gap-2 text-fg-muted text-sm">
                          <Icon icon={CircleCheck} class="size-4 shrink-0 text-ok" />
                          {t.lastOk(formatDateTime(last().at))}
                        </p>
                      }
                    >
                      {(error) => (
                        <p
                          role="alert"
                          class="flex items-start gap-2 rounded-control border border-crit/40 bg-crit/10 px-3 py-2 text-fg text-sm"
                        >
                          <Icon icon={CircleAlert} class="mt-0.5 size-4 shrink-0 text-crit" />
                          <span>
                            <span class="font-medium">
                              {t.lastFailed(formatDateTime(last().at))}
                            </span>
                            {`: ${error()}`}
                          </span>
                        </p>
                      )}
                    </Show>
                  )}
                </Show>
                <Show
                  when={found().backups.length > 0}
                  fallback={<p class="text-fg-subtle text-sm">{t.none}</p>}
                >
                  <ul class="flex flex-col divide-y divide-edge rounded-control border border-edge">
                    <For each={found().backups}>
                      {(backup) => {
                        const when = () => formatDateTime(backup.taken_at);
                        return (
                          <li class="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-3 py-2">
                            <div class="min-w-0 flex-1 text-sm">
                              <p class="font-medium tabular-nums">{t.taken(when())}</p>
                              <p class="truncate text-fg-muted text-xs" title={backup.name}>
                                {`${formatBytes(backup.size_bytes)} · ${backup.name}`}
                              </p>
                            </div>
                            <Show when={found().extra_dir !== null}>
                              <Badge tone={backup.copied ? "ok" : "warn"}>
                                {backup.copied ? t.copied : t.notCopied}
                              </Badge>
                            </Show>
                            <Show when={props.admin}>
                              <a
                                href={api.controller.backups.fileUrl(backup.name)}
                                download={backup.name}
                                aria-label={t.downloadLabel(when())}
                                class="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-control border border-edge bg-surface px-2.5 font-medium text-fg text-xs hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
                              >
                                <Icon icon={Download} class="size-3.5" />
                                {t.download}
                              </a>
                            </Show>
                          </li>
                        );
                      }}
                    </For>
                  </ul>
                </Show>
                <p class="text-fg-subtle text-xs">
                  {found().extra_dir
                    ? `${t.dir(found().dir)} ${t.extraDir(found().extra_dir ?? "")}.`
                    : `${t.dir(found().dir)}. ${t.noExtraDir}`}
                </p>
                <Show when={!props.admin}>
                  <p class="text-fg-subtle text-xs">{t.adminOnly}</p>
                </Show>
              </div>
            </Card>
            <Show when={props.admin}>
              <PassphraseCard set={found().passphrase_set} onSaved={() => void status.refetch()} />
              <Card aria-label={t.restore.title}>
                <CardHeader
                  title={t.restore.title}
                  description={t.restore.hint}
                  actions={
                    <Button
                      size="sm"
                      variant="secondary"
                      icon={ArchiveRestore}
                      onClick={() => setRestoring(true)}
                    >
                      {found().restore.state === "none" ? t.restore.action : t.restore.resume}
                    </Button>
                  }
                />
                <Show when={found().restore.state !== "none"}>
                  <p class="px-4 pb-4 text-fg-muted text-sm">{t.restore.staged}</p>
                </Show>
              </Card>
              <ControllerRestore
                open={restoring()}
                onOpenChange={setRestoring}
                onChanged={() => void status.refetch()}
              />
            </Show>
          </>
        )}
      </Show>
    </div>
  );
}
