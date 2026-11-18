import { createSignal } from "solid-js";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import { showToast } from "../../../ui/Toast";

const t = strings.settings.backups.passphrase;

/** The server's shortest passphrase (`MIN_PASSPHRASE`, D-129). */
export const MIN_PASSPHRASE = 12;

/** Sets or changes the backups' passphrase (admins; sealed in the vault, never read back). */
export function PassphraseCard(props: { set: boolean; onSaved: () => void }) {
  const [passphrase, setPassphrase] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [touched, setTouched] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const short = () => passphrase().length < MIN_PASSPHRASE;
  const mismatch = () => touched() && confirm() !== passphrase();

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    setTouched(true);
    if (short() || confirm() !== passphrase()) return;
    setSaving(true);
    setError(undefined);
    try {
      await api.controller.backups.passphrase({ passphrase: passphrase() });
      setPassphrase("");
      setConfirm("");
      setTouched(false);
      showToast({ title: t.saved, tone: "ok" });
      props.onSaved();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={props.set ? t.hintSet : t.hintUnset} />
      <form class="flex flex-col gap-4 px-4 pb-4" onSubmit={submit}>
        <RequestError text={error()} />
        <div class="grid gap-4 sm:grid-cols-2">
          <TextField
            label={t.label}
            name="backup-passphrase"
            type="password"
            autocomplete="new-password"
            value={passphrase()}
            onInput={setPassphrase}
            hint={t.minChars(MIN_PASSPHRASE)}
            error={touched() && short() ? t.minChars(MIN_PASSPHRASE) : undefined}
            minlength={MIN_PASSPHRASE}
            required
          />
          <TextField
            label={t.confirm}
            name="backup-passphrase-confirm"
            type="password"
            autocomplete="new-password"
            value={confirm()}
            onInput={setConfirm}
            error={mismatch() ? t.mismatch : undefined}
            required
          />
        </div>
        <div class="flex justify-end">
          <Button type="submit" variant={props.set ? "secondary" : "primary"} disabled={saving()}>
            {props.set ? t.change : t.set}
          </Button>
        </div>
      </form>
    </Card>
  );
}
