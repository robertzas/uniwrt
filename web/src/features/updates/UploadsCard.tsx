import { FileUp, Trash2 } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { FirmwareUpload } from "../../api/gen/FirmwareUpload";
import { api, errorText } from "../../lib/api";
import { formatBytes, formatTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";

const t = strings.firmware.uploads;

/**
 * The images admins uploaded (D-122): each with the boards it fits and how many devices that
 * is; admins upload (the controller reads its metadata and refuses what fits no board) and
 * delete. `onChange` reads the catalog again.
 */
export function UploadsCard(props: {
  uploads: readonly FirmwareUpload[];
  /** How many devices each upload fits, by SHA-256. */
  fits: (sha256: string) => number;
  admin: boolean;
  onChange: () => void;
}) {
  const [uploading, setUploading] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [confirming, setConfirming] = createSignal<string>();

  const upload = async (file: File | undefined) => {
    if (!file) return;
    setUploading(true);
    setError(undefined);
    try {
      await api.firmware.uploads.create(file);
      props.onChange();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setUploading(false);
    }
  };
  const remove = async (sha256: string) => {
    setError(undefined);
    try {
      await api.firmware.uploads.remove(sha256);
      setConfirming(undefined);
      props.onChange();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Card class="mx-auto w-full max-w-7xl" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin}>
            <label
              class={[
                "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-control border border-edge bg-surface px-3 font-medium text-fg text-sm hover:bg-raised focus-within:ring-2 focus-within:ring-accent/40",
                uploading() && "pointer-events-none opacity-60",
              ]}
            >
              <Icon icon={FileUp} class="size-4" />
              {uploading() ? t.uploading : t.upload}
              <input
                type="file"
                accept=".bin,.img,.gz,.itb"
                class="sr-only"
                disabled={uploading()}
                onChange={(event) => {
                  const input = event.currentTarget;
                  void upload(input.files?.[0]).then(() => {
                    input.value = "";
                  });
                }}
              />
            </label>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <RequestError text={error()} />
        <Show
          when={props.uploads.length > 0}
          fallback={<p class="text-fg-subtle text-sm">{t.empty}</p>}
        >
          <ul class="flex flex-col gap-2" aria-label={t.title}>
            <For each={props.uploads}>
              {(upload) => (
                <li
                  aria-label={upload.name}
                  class="flex flex-wrap items-center gap-x-4 gap-y-1.5 rounded-control border border-edge bg-canvas px-3 py-2.5"
                >
                  <div class="min-w-0 flex-1">
                    <p class="truncate font-medium text-fg text-sm" title={upload.name}>
                      {upload.name}
                    </p>
                    <p class="text-fg-muted text-xs">
                      {[
                        upload.version && `${upload.distribution ?? "OpenWrt"} ${upload.version}`,
                        upload.revision,
                        upload.boards.length > 0
                          ? upload.boards.join(", ")
                          : t.anyBoard(upload.target),
                        formatBytes(upload.size_bytes),
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </p>
                    <p class="text-fg-subtle text-xs">
                      {t.fits(props.fits(upload.sha256))} ·{" "}
                      {t.uploadedAt(formatTime(upload.uploaded_at))}
                    </p>
                  </div>
                  <Show when={props.admin}>
                    <Show
                      when={confirming() === upload.sha256}
                      fallback={
                        <Button
                          size="sm"
                          variant="ghost"
                          icon={Trash2}
                          aria-label={`${t.delete} ${upload.name}`}
                          onClick={() => setConfirming(upload.sha256)}
                        >
                          {t.delete}
                        </Button>
                      }
                    >
                      <span class="text-crit text-sm">{t.confirm(upload.name)}</span>
                      <Button size="sm" variant="ghost" onClick={() => setConfirming(undefined)}>
                        {t.cancel}
                      </Button>
                      <Button size="sm" class="text-crit" onClick={() => remove(upload.sha256)}>
                        {t.delete}
                      </Button>
                    </Show>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </Card>
  );
}
