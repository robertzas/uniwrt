import { createSignal, createUniqueId, For, Show } from "solid-js";
import type { ForwardProto } from "../../api/gen/ForwardProto";
import type { PortForward } from "../../api/gen/PortForward";
import type { SiteView } from "../../api/gen/SiteView";
import { ApiError, errorText } from "../../lib/api";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Checkbox } from "../../ui/Checkbox";
import { Dialog } from "../../ui/Dialog";
import { FormField, INPUT, SELECT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import {
  type ForwardDraft,
  type ForwardErrors,
  forwardDraft,
  forwardErrorsOfIssues,
  forwardOf,
  withForward,
} from "./forwards";

const t = strings.security.forward;
const PROTOS: ForwardProto[] = ["tcp", "udp", "tcp_udp"];

/** A host the form suggests: a client's IPv4 address and its name. */
export interface HostChoice {
  ip: string;
  name: string;
}

/**
 * A port forward's editor (network.md, Firewall policy): the form checks what the server
 * checks, a save is the site's next revision, and the server's issues land on their fields.
 */
export function ForwardDialog(props: {
  view: SiteView;
  /** The forward to edit; a new one without. */
  forward: PortForward | undefined;
  hosts: readonly HostChoice[];
  onClose: () => void;
}) {
  const [draft, setDraft] = createSignal<ForwardDraft>(forwardDraft(props.forward));
  const [errors, setErrors] = createSignal<ForwardErrors>({});
  const [other, setOther] = createSignal<string[]>([]);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [deleting, setDeleting] = createSignal(false);
  const hostList = createUniqueId();
  const original = () => props.forward?.name;
  const others = () => props.view.config.firewall.forwards.filter((f) => f.name !== original());
  const change = (next: Partial<ForwardDraft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setOther([]);
  };

  const store = async (forward: PortForward | null, message: string) => {
    setBusy(true);
    setFailure(undefined);
    let index = -1;
    try {
      const view = await saveSite((config) => {
        const next = withForward(config, original(), forward);
        index = forward ? next.firewall.forwards.findIndex((f) => f.name === forward.name) : -1;
        return next;
      }, message);
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const mapped = forwardErrorsOfIssues(
          err.issues.filter((issue) => issue.severity === "error"),
          index,
        );
        setErrors(mapped.errors);
        setOther(mapped.other);
        setFailure(err.message);
      } else {
        setFailure(errorText(err));
      }
    } finally {
      setBusy(false);
    }
  };

  const save = () => {
    const { forward, errors: found } = forwardOf(draft(), others(), props.view.config.networks);
    if (!forward) {
      setErrors(found);
      return;
    }
    void store(forward, props.forward ? t.message(forward.name) : t.messageNew(forward.name));
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={props.forward ? t.title(props.forward.name) : t.titleNew}
      size="lg"
    >
      <Show
        when={saved() === undefined}
        fallback={
          <div class="flex flex-col gap-4 px-5 pb-5">
            <SavedRevision revision={saved() ?? 0} />
            <div class="flex justify-end">
              <Button onClick={() => props.onClose()}>{strings.security.close}</Button>
            </div>
          </div>
        }
      >
        <form
          class="flex flex-col gap-5 px-5 pt-1 pb-5"
          onSubmit={(event) => {
            event.preventDefault();
            save();
          }}
        >
          <div class="grid gap-4 sm:grid-cols-2">
            <Field
              id={`${hostList}-name`}
              label={t.name}
              hint={t.nameHint}
              error={errors().name}
              value={draft().name}
              mono={false}
              onInput={(name) => change({ name })}
            />
            <FormField label={t.proto} for={`${hostList}-proto`}>
              <select
                id={`${hostList}-proto`}
                class={SELECT}
                value={draft().proto}
                onChange={(event) => change({ proto: event.currentTarget.value as ForwardProto })}
              >
                <For each={PROTOS}>
                  {(proto) => (
                    <option value={proto}>{strings.security.forwards.proto[proto]}</option>
                  )}
                </For>
              </select>
            </FormField>
            <Field
              id={`${hostList}-ports`}
              label={t.ports}
              hint={t.portsHint}
              error={errors().ports}
              value={draft().ports}
              onInput={(ports) => change({ ports })}
            />
            <Field
              id={`${hostList}-source`}
              label={t.source}
              hint={t.sourceHint}
              error={errors().source}
              value={draft().source}
              onInput={(source) => change({ source })}
            />
            <Field
              id={`${hostList}-to`}
              label={t.to}
              hint={t.toHint}
              error={errors().to}
              value={draft().to}
              list={hostList}
              onInput={(to) => change({ to })}
            />
            <Field
              id={`${hostList}-to-port`}
              label={t.toPort}
              hint={t.toPortHint}
              error={errors().to_port}
              value={draft().toPort}
              onInput={(toPort) => change({ toPort })}
            />
          </div>
          <datalist id={hostList}>
            <For each={props.hosts}>{(host) => <option value={host.ip}>{host.name}</option>}</For>
          </datalist>
          <div class="flex flex-col gap-3">
            <Checkbox
              label={t.enabled}
              hint={t.enabledHint}
              checked={draft().enabled}
              onChange={(enabled) => change({ enabled })}
            />
            <Checkbox
              label={t.reflection}
              hint={t.reflectionHint}
              checked={draft().reflection}
              onChange={(reflection) => change({ reflection })}
            />
          </div>
          <Show when={failure()}>
            <div role="alert">
              <RequestError text={failure()} />
              <For each={other()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
            </div>
          </Show>
          <Show when={deleting() && props.forward}>
            {(forward) => (
              <div
                class="rounded-control border border-crit/40 bg-crit/10 px-3 py-2.5 text-sm"
                role="alert"
              >
                <p>{t.confirmDelete(forward().name)}</p>
                <div class="mt-2 flex gap-2">
                  <Button
                    size="sm"
                    variant="primary"
                    disabled={busy()}
                    onClick={() => void store(null, t.messageDelete(forward().name))}
                  >
                    {t.delete}
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setDeleting(false)}>
                    {strings.security.cancel}
                  </Button>
                </div>
              </div>
            )}
          </Show>
          <div class="flex flex-wrap items-center gap-2">
            <Show when={props.forward && !deleting()}>
              <Button variant="ghost" onClick={() => setDeleting(true)} disabled={busy()}>
                {t.delete}
              </Button>
            </Show>
            <span class="flex-1" />
            <Button variant="ghost" onClick={() => props.onClose()} disabled={busy()}>
              {strings.security.cancel}
            </Button>
            <Button type="submit" variant="primary" disabled={busy()}>
              {strings.security.save}
            </Button>
          </div>
        </form>
      </Show>
    </Dialog>
  );
}

/** A text input, monospace for ports and addresses, optionally with suggestions. */
function Field(props: {
  id: string;
  label: string;
  hint: string;
  error: string | undefined;
  value: string;
  list?: string;
  mono?: boolean;
  onInput: (value: string) => void;
}) {
  return (
    <FormField label={props.label} for={props.id} hint={props.hint} error={props.error}>
      <input
        id={props.id}
        list={props.list}
        aria-invalid={props.error ? "true" : undefined}
        class={[
          INPUT,
          props.mono !== false && "font-mono",
          props.error ? "border-crit" : "border-edge",
        ]}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
      />
    </FormField>
  );
}
