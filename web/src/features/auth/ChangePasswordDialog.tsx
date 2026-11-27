import { createSignal } from "solid-js";
import { api, errorText } from "../../lib/api";
import { MIN_PASSWORD_CHARS, newPasswordError } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog, DialogFooter } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { TextField } from "../../ui/TextField";
import { showToast } from "../../ui/Toast";

const t = strings.account.password;

/**
 * Changes the signed-in user's own password (security.md, Users; D-153): the current one, then
 * the new one twice. The user's other sessions end; this one stays.
 */
export function ChangePasswordDialog(props: { onClose: () => void }) {
  const [current, setCurrent] = createSignal("");
  const [next, setNext] = createSignal("");
  const [again, setAgain] = createSignal("");
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [nextError, setNextError] = createSignal<string>();
  const [againError, setAgainError] = createSignal<string>();

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (pending()) return;
    setError(undefined);
    const short = newPasswordError(next(), t.tooShort);
    const mismatch = !short && again() !== next() ? t.mismatch : undefined;
    setNextError(short);
    setAgainError(mismatch);
    if (short || mismatch) return;
    setPending(true);
    try {
      await api.auth.password({ current: current(), new: next() });
      showToast({ title: t.done, description: t.doneHint, tone: "ok" });
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={t.title}
      description={t.description}
    >
      <form class="flex flex-col gap-4 px-5 pt-2 pb-4" onSubmit={submit}>
        <RequestError text={error()} />
        <TextField
          label={t.current}
          name="current-password"
          type="password"
          autocomplete="current-password"
          value={current()}
          onInput={setCurrent}
          required
        />
        <TextField
          label={t.new}
          name="new-password"
          type="password"
          autocomplete="new-password"
          value={next()}
          onInput={(value) => {
            setNext(value);
            setNextError(undefined);
          }}
          hint={t.minChars(MIN_PASSWORD_CHARS)}
          error={nextError()}
          required
        />
        <TextField
          label={t.confirm}
          name="confirm-password"
          type="password"
          autocomplete="new-password"
          value={again()}
          onInput={(value) => {
            setAgain(value);
            setAgainError(undefined);
          }}
          error={againError()}
          required
        />
        <DialogFooter class="pt-2">
          <Button variant="ghost" type="button" onClick={() => props.onClose()}>
            {t.cancel}
          </Button>
          <Button variant="primary" type="submit" disabled={pending()}>
            {pending() ? t.pending : t.submit}
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}
