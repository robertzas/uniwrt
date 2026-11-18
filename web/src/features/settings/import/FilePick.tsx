import { FileUp, LoaderCircle } from "lucide";
import { createSignal, Show } from "solid-js";
import { formatDateTime } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { FormField } from "../../../ui/FormField";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import { type PickedFile, readSiteFile } from "./file";

const t = strings.settings.fileImport;

/**
 * The file import's first step: the file (read and checked as a site file here), then its
 * passphrase when it holds secrets (empty imports without them), then *Preview*. `picked`
 * survives a return from the preview, to enter the passphrase it lacked.
 */
export function FilePick(props: {
  picked: PickedFile | undefined;
  onPick: (file: PickedFile | undefined) => void;
  passphrase: string;
  onPassphrase: (value: string) => void;
  busy: boolean;
  error: string | undefined;
  onPreview: () => void;
}) {
  const [readError, setReadError] = createSignal<string>();

  const choose = async (file: File | undefined) => {
    setReadError(undefined);
    props.onPick(undefined);
    props.onPassphrase("");
    if (!file) return;
    const read = readSiteFile(file.name, await file.text());
    if ("error" in read) setReadError(read.error);
    else props.onPick(read);
  };

  return (
    <Card aria-label={t.crumb}>
      <form
        class="flex flex-col gap-4 px-4 py-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (props.picked && !props.busy) props.onPreview();
        }}
      >
        <FormField label={t.file} for="site-file" hint={t.choose} error={readError()}>
          <input
            id="site-file"
            type="file"
            accept=".json,application/json"
            disabled={props.busy}
            class="text-sm file:mr-3 file:h-9 file:cursor-pointer file:rounded-control file:border file:border-edge file:bg-surface file:px-3 file:font-medium file:text-fg file:text-sm hover:file:bg-raised"
            onChange={(event) => choose(event.currentTarget.files?.[0])}
          />
        </FormField>
        <Show when={props.picked}>
          {(file) => (
            <div class="flex flex-col gap-4 rounded-control border border-edge px-3 py-3">
              <div class="flex items-start gap-2.5 text-sm">
                <Icon icon={FileUp} class="mt-0.5 size-4 shrink-0 text-fg-subtle" />
                <div class="min-w-0">
                  <p class="break-all font-medium">{file().name}</p>
                  <p class="text-fg-muted">
                    {t.read(file().revision, file().uniwrt, formatDateTime(file().exportedAt))}
                  </p>
                  <p class="text-fg-muted">{file().secrets ? t.holdsSecrets : t.noSecrets}</p>
                </div>
              </div>
              <Show when={file().secrets}>
                <TextField
                  label={t.passphrase}
                  name="site-file-passphrase"
                  type="password"
                  autocomplete="off"
                  value={props.passphrase}
                  onInput={props.onPassphrase}
                  hint={t.passphraseHint}
                  disabled={props.busy}
                  autofocus
                />
              </Show>
            </div>
          )}
        </Show>
        <RequestError text={props.error ? `${t.failed}: ${props.error}` : undefined} />
        <div class="flex items-center justify-end gap-3">
          <Show when={props.busy}>
            <p role="status" class="flex items-center gap-2 text-fg-muted text-sm">
              <Icon icon={LoaderCircle} class="size-4 animate-spin" />
              {t.previewing}
            </p>
          </Show>
          <Button type="submit" variant="primary" disabled={!props.picked || props.busy}>
            {t.preview}
          </Button>
        </div>
      </form>
    </Card>
  );
}
