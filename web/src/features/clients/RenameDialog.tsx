import { createEffect, createSignal } from "solid-js";
import { ApiError, api } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { TextField } from "../../ui/TextField";
import { showToast } from "../../ui/Toast";

const r = strings.client.renameDialog;

/** Gives the client an alias (or takes it away); the live view shows the new name. */
export function RenameDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mac: string;
  alias: string | null;
  placeholder: string;
}) {
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal<string>();
  const [saving, setSaving] = createSignal(false);
  // The alias is read with `open`, so the field starts from it each time the dialog opens.
  createEffect(
    () => (props.open ? (props.alias ?? "") : null),
    (initial) => {
      if (initial !== null) {
        setName(initial);
        setError(undefined);
      }
    },
  );
  const save = async (event: SubmitEvent) => {
    event.preventDefault();
    setSaving(true);
    try {
      await api.clients.rename(props.mac, { alias: name().trim() || null });
      showToast({ title: r.saved, tone: "ok" });
      props.onOpenChange(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : r.failed);
    } finally {
      setSaving(false);
    }
  };
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={r.title}
      description={r.description}
    >
      <form class="flex flex-col gap-4 px-5 pt-2 pb-5" onSubmit={save}>
        <TextField
          label={r.label}
          value={name()}
          onInput={setName}
          hint={r.hint}
          error={error()}
          autofocus
        />
        <div class="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={() => props.onOpenChange(false)}>
            {r.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={saving()}>
            {r.save}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
