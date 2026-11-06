import { Plus, Trash2 } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { DnsRecord } from "../../api/gen/DnsRecord";
import type { SiteView } from "../../api/gen/SiteView";
import { ApiError, errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { INPUT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import {
  type RecordDraft,
  type RecordErrors,
  recordDraft,
  recordErrorsOfIssues,
  recordsOf,
} from "./dns";

const t = strings.networks.dns;

/**
 * Local DNS (network.md, DHCP & local DNS): the custom A/AAAA and CNAME records besides the
 * automatic `<hostname>.<domain>` and `uniwrt.<domain>`; admins edit them as one revision.
 */
export function DnsCard(props: { view: SiteView; admin: boolean }) {
  const [drafts, setDrafts] = createSignal<RecordDraft[]>();
  const [errors, setErrors] = createSignal<RecordErrors[]>([]);
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const domain = () => props.view.config.site.domain;
  const records = () => props.view.config.services.dns_records;

  const edit = () => {
    setDrafts(records().map(recordDraft));
    setErrors([]);
    setOther([]);
    setFailure(undefined);
    setSaved(undefined);
  };
  const update = (index: number, next: Partial<RecordDraft>) => {
    setDrafts((now) => now?.map((d, i) => (i === index ? { ...d, ...next } : d)));
    setErrors([]);
  };

  const save = async () => {
    const now = drafts();
    if (!now) return;
    const { records: next, errors: found } = recordsOf(now, domain());
    if (!next) {
      setErrors(found);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, services: { ...config.services, dns_records: next } }),
        t.message,
      );
      setSaved(view.revision.id);
      setDrafts(undefined);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = recordErrorsOfIssues(err.issues, now.length);
        setErrors(mapped.errors);
        setOther(mapped.other);
      }
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description(domain())}
        actions={
          <Show when={props.admin && !drafts()}>
            <Button size="sm" onClick={edit}>
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={drafts()}
          fallback={
            <Show
              when={records().length > 0}
              fallback={<p class="text-fg-muted text-sm">{t.none}</p>}
            >
              <dl class="grid grid-cols-[max-content_max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
                <For each={records()}>
                  {(record: DnsRecord) => (
                    <>
                      <dt class="font-mono text-xs leading-5">{record.name}</dt>
                      <dd class="text-fg-muted text-xs leading-5">{t.kind[record.kind]}</dd>
                      <dd class="min-w-0 truncate font-mono text-xs leading-5">
                        {record.kind === "host" ? record.ip : record.target}
                      </dd>
                    </>
                  )}
                </For>
              </dl>
            </Show>
          }
        >
          {(rows) => (
            <form
              class="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              <For each={rows()} keyed={false}>
                {(row, i) => (
                  <div class="grid gap-2 sm:grid-cols-[10rem_1fr_1fr_auto]">
                    <select
                      aria-label={t.kindLabel}
                      class={[INPUT, "border-edge"]}
                      value={row().kind}
                      onChange={(event) =>
                        update(i, { kind: event.currentTarget.value as RecordDraft["kind"] })
                      }
                    >
                      <option value="host">{t.kind.host}</option>
                      <option value="cname">{t.kind.cname}</option>
                    </select>
                    <Input
                      label={t.name}
                      value={row().name}
                      error={errors()[i]?.name}
                      onInput={(name) => update(i, { name })}
                    />
                    <Input
                      label={row().kind === "host" ? t.ip : t.target}
                      value={row().value}
                      error={errors()[i]?.value}
                      onInput={(value) => update(i, { value })}
                    />
                    <Button
                      variant="ghost"
                      icon={Trash2}
                      aria-label={t.remove(row().name || String(i + 1))}
                      onClick={() => {
                        setDrafts((now) => now?.filter((_, j) => j !== i));
                        setErrors([]);
                      }}
                    />
                  </div>
                )}
              </For>
              <div class="flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  icon={Plus}
                  onClick={() =>
                    setDrafts((now) => [...(now ?? []), { kind: "host", name: "", value: "" }])
                  }
                >
                  {t.add}
                </Button>
                <span class="flex-1" />
                <Button variant="ghost" onClick={() => setDrafts(undefined)} disabled={busy()}>
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
        <For each={other()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
    </Card>
  );
}

function Input(props: {
  label: string;
  value: string;
  error: string | undefined;
  onInput: (value: string) => void;
}) {
  return (
    <div class="min-w-0">
      <input
        aria-label={props.label}
        placeholder={props.label}
        aria-invalid={props.error ? "true" : undefined}
        class={[INPUT, "font-mono", props.error ? "border-crit" : "border-edge"]}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
      />
      <Show when={props.error}>
        <p class="mt-1 text-crit text-xs">{props.error}</p>
      </Show>
    </div>
  );
}
