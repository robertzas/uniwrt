import { Gauge } from "lucide";
import { createEffect, createSignal, For, Show } from "solid-js";
import type { Sqm } from "../../api/gen/Sqm";
import { errorText } from "../../lib/api";
import { formatRate } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { FormField, INPUT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { TextField } from "../../ui/TextField";
import { OVERHEADS, type OverheadKey, type SqmDraft, saveWan, sqmDraft, sqmOf } from "./uplink";

const t = strings.internet;

/**
 * SQM on the Internet page (wan.md, SQM; D-076): what cake shapes, and for admins the form,
 * saved as a revision. `suggested` fills the rates from a test of the line.
 */
export function SqmCard(props: {
  sqm: Sqm | null;
  admin: boolean;
  gateway: number | undefined;
  /** Rates a test of the line suggests, to start an edit from: (download, upload) kbit/s. */
  suggested: [number, number] | undefined;
  /** Bumped when the user takes the suggestion, so the form opens with it. */
  take: number;
}) {
  const [draft, setDraft] = createSignal<SqmDraft>();
  const [errors, setErrors] = createSignal<{ down?: string; up?: string }>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<{ revision: number; installs: boolean }>();

  const edit = (from?: [number, number]) => {
    const start = sqmDraft(props.sqm);
    setDraft(
      from
        ? { ...start, on: true, down: String(from[0] / 1000), up: String(from[1] / 1000) }
        : start,
    );
    setErrors({});
    setFailure(undefined);
    setSaved(undefined);
  };
  // Taking the suggestion opens the form with its rates.
  createEffect(
    () => props.take,
    (take) => {
      if (take > 0 && props.suggested) edit(props.suggested);
    },
  );
  const change = (next: Partial<SqmDraft>) => {
    setDraft((now) => (now ? { ...now, ...next } : now));
    setErrors({});
  };

  const save = async () => {
    const now = draft();
    if (!now) return;
    const { sqm, errors: found } = sqmOf(now, props.sqm);
    if (Object.keys(found).length > 0) {
      setErrors(found);
      return;
    }
    setBusy(true);
    try {
      const revision = await saveWan(
        (wan) => ({ ...wan, sqm }),
        sqm ? t.sqm.message : t.sqm.messageOff,
        props.gateway,
      );
      setSaved({ revision, installs: sqm !== null && props.sqm === null });
      setDraft(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.sqm.title}>
      <CardHeader
        title={t.sqm.title}
        description={t.sqm.description}
        actions={
          <Show when={props.admin && !draft()}>
            <Button size="sm" onClick={() => edit()}>
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={draft()}
          fallback={
            <Show when={props.sqm} fallback={<p class="text-fg-muted text-sm">{t.sqm.off}</p>}>
              {(sqm) => (
                <div class="flex flex-wrap items-center gap-2 text-sm">
                  <Badge tone="ok" icon={Gauge}>
                    {t.sqm.shaping(formatRate(sqm().download_kbps), formatRate(sqm().upload_kbps))}
                  </Badge>
                  <Show when={sqm().overhead !== null}>
                    <span class="text-fg-muted text-xs">
                      {t.sqm.overhead}: {sqm().overhead} B
                    </span>
                  </Show>
                  <Show when={sqm().autotune}>
                    <span class="text-fg-muted text-xs">{t.sqm.autotuneOn}</span>
                  </Show>
                </div>
              )}
            </Show>
          }
        >
          {(form) => (
            <form
              class="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              <Checkbox
                checked={form().on}
                onChange={(on) => change({ on })}
                label={t.sqm.enable}
              />
              <Show when={form().on}>
                <div class="grid gap-3 sm:grid-cols-3">
                  <TextField
                    label={t.sqm.download}
                    value={form().down}
                    onInput={(down) => change({ down })}
                    error={errors().down}
                  />
                  <TextField
                    label={t.sqm.upload}
                    value={form().up}
                    onInput={(up) => change({ up })}
                    error={errors().up}
                  />
                  <FormField label={t.sqm.overhead} for="sqm-overhead">
                    <select
                      id="sqm-overhead"
                      class={[INPUT, "border-edge"]}
                      value={form().overhead}
                      onChange={(event) =>
                        change({ overhead: event.currentTarget.value as OverheadKey })
                      }
                    >
                      <For each={Object.keys(OVERHEADS)}>
                        {(key) => <option value={key}>{t.sqm.overheads[key]}</option>}
                      </For>
                    </select>
                  </FormField>
                </div>
                <Checkbox
                  checked={form().autotune}
                  onChange={(autotune) => change({ autotune })}
                  label={t.sqm.autotune}
                />
              </Show>
              <div class="flex justify-end gap-2">
                <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
                  {t.cancel}
                </Button>
                <Button type="submit" variant="primary" disabled={busy()}>
                  {t.save}
                </Button>
              </div>
            </form>
          )}
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>
          {(done) => (
            <SavedRevision
              revision={done().revision}
              note={done().installs ? t.installs : undefined}
            />
          )}
        </Show>
      </div>
    </Card>
  );
}
