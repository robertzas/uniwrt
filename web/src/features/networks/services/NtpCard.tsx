import { createSignal, Show } from "solid-js";
import type { SiteView } from "../../../api/gen/SiteView";
import { errorText } from "../../../lib/api";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";

const t = strings.networks.ntp;

/**
 * The gateway's NTP server (network.md, Other services; D-132): on, it answers NTP and pools
 * without time servers of their own hand it out. Saved as one revision.
 */
export function NtpCard(props: { view: SiteView; admin: boolean }) {
  const [draft, setDraft] = createSignal<boolean>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const current = () => props.view.config.services.ntp_server;
  const changed = () => draft() !== undefined && draft() !== current();

  const save = async () => {
    const next = draft();
    if (next === undefined) return;
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, services: { ...config.services, ntp_server: next } }),
        t.message(next),
      );
      setSaved(view.revision.id);
      setDraft(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Checkbox
          checked={draft() ?? current()}
          onChange={(on) => {
            setDraft(on);
            setSaved(undefined);
          }}
          label={t.label}
          hint={t.hint}
          disabled={!props.admin || busy()}
        />
        <Show when={changed()}>
          <div class="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
              {t.cancel}
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={busy()}>
              {t.save}
            </Button>
          </div>
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
    </Card>
  );
}
