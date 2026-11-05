import { useNavigate } from "@solidjs/router";
import { createSignal, Show } from "solid-js";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { JobSuccessNote, RequestError } from "../../../ui/JobOutcome";

const t = strings.settings.revisions.restore;

/**
 * Restoring revision `id` (lifecycle.md, Desired state & revisions): confirmed, it becomes the
 * next revision on `base` (the latest), and the dialog then offers its review and apply.
 */
export function RestoreDialog(props: {
  id: number;
  base: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const navigate = useNavigate();
  const [sending, setSending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [restored, setRestored] = createSignal<number>();

  const close = (open: boolean) => {
    if (!open) {
      setError(undefined);
      setRestored(undefined);
    }
    props.onOpenChange(open);
  };

  const restore = async () => {
    setSending(true);
    setError(undefined);
    try {
      const view = await api.revisions.restore(props.id, { base: props.base });
      setRestored(view.revision.id);
    } catch (err) {
      setError(`${t.failed}: ${errorText(err) ?? ""}`);
    } finally {
      setSending(false);
    }
  };

  return (
    <Dialog open={props.open} onOpenChange={close} title={t.title(props.id)} description={t.body}>
      <div class="flex flex-col gap-3 px-5 pt-2 pb-5">
        <RequestError text={error()} />
        <Show
          when={restored()}
          fallback={
            <div class="flex justify-end gap-2">
              <Button variant="ghost" onClick={() => close(false)}>
                {t.cancel}
              </Button>
              <Button variant="primary" disabled={sending()} onClick={restore}>
                {t.confirm}
              </Button>
            </div>
          }
        >
          {(as) => (
            <>
              <JobSuccessNote text={t.done(props.id, as())} />
              <div class="flex justify-end gap-2">
                <Button variant="ghost" onClick={() => close(false)}>
                  {strings.common.close}
                </Button>
                <Button variant="primary" onClick={() => navigate("/changes")}>
                  {t.review}
                </Button>
              </div>
            </>
          )}
        </Show>
      </div>
    </Dialog>
  );
}
