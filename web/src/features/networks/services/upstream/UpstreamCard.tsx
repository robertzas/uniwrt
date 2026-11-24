import { createSignal, For, Show } from "solid-js";
import type { ServiceFacts } from "../../../../api/gen/ServiceFacts";
import type { SiteView } from "../../../../api/gen/SiteView";
import { ApiError, errorText } from "../../../../lib/api";
import { saveSite } from "../../../../lib/site";
import { strings } from "../../../../lib/strings";
import { Button } from "../../../../ui/Button";
import { Card, CardHeader } from "../../../../ui/Card";
import { RequestError } from "../../../../ui/JobOutcome";
import { SavedRevision } from "../../../../ui/SavedRevision";
import { NO_ERRORS, type UpstreamErrors, upstreamErrorsOfIssues, upstreamOf } from "./check";
import { describeUpstream, type UpstreamDraft, upstreamDraft } from "./draft";
import { UpstreamEditor } from "./Editor";
import { UpstreamSummary } from "./Summary";

const t = strings.networks.upstream;

/**
 * Upstream DNS (network.md, *Upstream DNS*; D-132, D-146): the layers the gateway's dnsmasq
 * asks in turn (encrypted resolvers over HTTPS or TLS, plain servers, the ISP's) and the
 * proxies the gateway reports running. Admins edit it in place and save it as one revision.
 */
export function UpstreamCard(props: {
  view: SiteView;
  admin: boolean;
  services: ServiceFacts | undefined;
}) {
  const [draft, setDraft] = createSignal<UpstreamDraft>();
  const [errors, setErrors] = createSignal<UpstreamErrors>(NO_ERRORS);
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const upstream = () => props.view.config.services.upstream;

  const edit = () => {
    setDraft(upstreamDraft(upstream()));
    setErrors(NO_ERRORS);
    setOther([]);
    setFailure(undefined);
    setSaved(undefined);
  };
  const change = (next: (now: UpstreamDraft) => Partial<UpstreamDraft>) => {
    setDraft((now) => now && { ...now, ...next(now) });
    setErrors(NO_ERRORS);
    setOther([]);
  };

  const save = async () => {
    const now = draft();
    if (!now) return;
    const { upstream: next, errors: found } = upstreamOf(now);
    if (!next) {
      setErrors(found);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, services: { ...config.services, upstream: next } }),
        t.message(describeUpstream(next)),
      );
      setSaved(view.revision.id);
      setDraft(undefined);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = upstreamErrorsOfIssues(err.issues.filter((i) => i.severity === "error"));
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
        description={t.description}
        actions={
          <Show when={props.admin && !draft()}>
            <Button size="sm" onClick={edit}>
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={draft()}
          fallback={<UpstreamSummary upstream={upstream()} services={props.services} />}
        >
          {(now) => (
            <form
              class="flex flex-col gap-5"
              onSubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              <UpstreamEditor draft={now()} errors={errors()} change={change} />
              <div class="flex flex-wrap items-center justify-end gap-2">
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
        <For each={other()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
    </Card>
  );
}
