import { createSignal, createUniqueId, For, Show } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { SiteView } from "../../../api/gen/SiteView";
import { errorText } from "../../../lib/api";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { FormField, SELECT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { TextField } from "../../../ui/TextField";
import { ADOPTION_ADDRESS, addressError, adoptionPorts, withAdoptionPort } from "./adoption";

const t = strings.adoptionPort;

/**
 * The site's adoption port (adoption.md, Path C; D-155), on the gateway's Ports tab: which of its
 * LAN ports is set aside for factory-default routers, and its address there, saved as the next
 * revision. Admins edit it.
 */
export function AdoptionPortCard(props: {
  node: Node;
  facts: NodeFacts | null;
  view: SiteView;
  admin: boolean;
}) {
  const id = createUniqueId();
  const stored = () => props.view.config.adoption_port;
  const [draft, setDraft] = createSignal<{ port: string; address: string }>();
  const [error, setError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const ports = () => adoptionPorts(props.facts, stored()?.port ?? null);
  /** The port's networks go when it becomes the adoption port. */
  const leaves = () => {
    const port = draft()?.port;
    return port && port !== stored()?.port ? port : undefined;
  };
  const issues = () =>
    props.view.issues.filter((i) => i.path.startsWith("adoption_port")).map((i) => i.message);

  const save = async () => {
    const now = draft();
    if (!now) return;
    const invalid = now.port === "" ? null : addressError(now.address);
    if (invalid) {
      setError(t.addressErrors[invalid]);
      return;
    }
    const port = now.port === "" ? null : { port: now.port, address: now.address.trim() };
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => withAdoptionPort(config, port),
        port ? t.message(port.port) : t.removed,
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
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && !draft() && (ports().length > 0 || stored())}>
            <Button
              size="sm"
              onClick={() => {
                setDraft({
                  port: stored()?.port ?? "",
                  address: stored()?.address ?? ADOPTION_ADDRESS,
                });
                setSaved(undefined);
                setFailure(undefined);
                setError(undefined);
              }}
            >
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={draft()}
          fallback={
            <p class="text-sm">
              <Show when={stored()} fallback={<span class="text-fg-muted">{t.off}</span>}>
                {(port) => t.current(port().port, port().address)}
              </Show>
            </p>
          }
        >
          {(now) => (
            <form
              class="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              <div class="grid gap-3 sm:grid-cols-2">
                <FormField label={t.port} for={id}>
                  <select
                    id={id}
                    class={SELECT}
                    value={now().port}
                    onChange={(event) => {
                      const port = event.currentTarget.value;
                      setDraft((was) => was && { ...was, port });
                    }}
                  >
                    <option value="">{t.none}</option>
                    <For each={ports()}>{(port) => <option value={port}>{port}</option>}</For>
                  </select>
                </FormField>
                <Show when={now().port !== ""}>
                  <TextField
                    label={t.address}
                    name="adoption-address"
                    value={now().address}
                    hint={t.addressHint}
                    error={error()}
                    onInput={(address) => {
                      setDraft((was) => was && { ...was, address });
                      setError(undefined);
                    }}
                  />
                </Show>
              </div>
              <Show when={leaves()}>
                {(port) => <p class="text-warn text-xs">{t.leaves(port())}</p>}
              </Show>
              <Show when={now().port !== ""}>
                <p class="text-fg-muted text-xs">{t.overlap}</p>
              </Show>
              <div class="flex flex-wrap justify-end gap-2">
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
        <Show when={ports().length === 0 && !stored()}>
          <p class="text-fg-muted text-xs">{t.noPorts}</p>
        </Show>
        <Show when={!draft()}>
          <For each={issues()}>{(issue) => <p class="text-warn text-xs">{issue}</p>}</For>
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      </div>
    </Card>
  );
}
