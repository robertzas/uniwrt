import { createSignal, Show } from "solid-js";
import { api, errorText } from "../../../lib/api";
import { saveFile } from "../../../lib/download";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { RadioCards } from "../../../ui/RadioCards";
import { TextField } from "../../../ui/TextField";
import { MIN_PASSPHRASE } from "../backups/PassphraseCard";

const t = strings.settings.revisions.export;

type Kind = "plain" | "secret";

/**
 * Exporting the site (lifecycle.md, Backups: Site export/import; D-130): the latest revision as
 * a JSON file, without its secrets (anyone) or, for admins, with them sealed under a passphrase
 * of at least `MIN_PASSPHRASE` characters, typed twice.
 */
export function ExportDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  admin: boolean;
}) {
  const [kind, setKind] = createSignal<Kind>("plain");
  const [passphrase, setPassphrase] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [touched, setTouched] = createSignal(false);
  const [sending, setSending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [saved, setSaved] = createSignal<string>();
  // Counted as the server counts: characters, not UTF-16 units.
  const short = () => [...passphrase()].length < MIN_PASSPHRASE;
  const secret = () => props.admin && kind() === "secret";

  const close = (open: boolean) => {
    if (!open) {
      setKind("plain");
      setPassphrase("");
      setConfirm("");
      setTouched(false);
      setError(undefined);
      setSaved(undefined);
    }
    props.onOpenChange(open);
  };

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (secret()) {
      setTouched(true);
      if (short() || confirm() !== passphrase()) return;
    }
    setSending(true);
    setError(undefined);
    try {
      const file = secret()
        ? await api.siteFile.exportSecrets({ passphrase: passphrase() })
        : await api.siteFile.export();
      saveFile(file);
      setSaved(file.name);
      setPassphrase("");
      setConfirm("");
      setTouched(false);
    } catch (err) {
      setError(`${t.failed}: ${errorText(err) ?? ""}`);
    } finally {
      setSending(false);
    }
  };

  return (
    <Dialog open={props.open} onOpenChange={close} title={t.title} description={t.body}>
      <form class="flex flex-col gap-4 px-5 pt-2 pb-5" onSubmit={submit}>
        <Show when={props.admin}>
          <RadioCards
            label={t.kind}
            value={kind()}
            onChange={(next) => {
              setKind(next);
              setSaved(undefined);
            }}
            disabled={sending()}
            options={[
              { value: "plain", label: t.plain, description: t.plainHint },
              { value: "secret", label: t.secret, description: t.secretHint },
            ]}
          />
        </Show>
        <Show when={secret()}>
          <div class="grid gap-4 sm:grid-cols-2">
            <TextField
              label={t.passphrase}
              name="export-passphrase"
              type="password"
              autocomplete="new-password"
              value={passphrase()}
              onInput={setPassphrase}
              hint={t.passphraseHint(MIN_PASSPHRASE)}
              error={touched() && short() ? t.tooShort(MIN_PASSPHRASE) : undefined}
              minlength={MIN_PASSPHRASE}
              disabled={sending()}
              required
            />
            <TextField
              label={t.confirm}
              name="export-passphrase-confirm"
              type="password"
              autocomplete="new-password"
              value={confirm()}
              onInput={setConfirm}
              error={touched() && confirm() !== passphrase() ? t.mismatch : undefined}
              disabled={sending()}
              required
            />
          </div>
        </Show>
        <RequestError text={error()} />
        <Show when={saved()}>{(name) => <JobSuccessNote text={t.done(name())} />}</Show>
        <div class="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => close(false)}>
            {saved() ? strings.common.close : t.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={sending()}>
            {sending() ? t.downloading : t.download}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
