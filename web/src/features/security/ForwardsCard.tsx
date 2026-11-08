import { ArrowRight, Plus, Trash2 } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { PortForward } from "../../api/gen/PortForward";
import type { SiteView } from "../../api/gen/SiteView";
import { ApiError, errorText } from "../../lib/api";
import { formatIpv4, ipv4 } from "../../lib/ipv4";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { INPUT } from "../../ui/FormField";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { ForwardDialog, type HostChoice } from "./ForwardDialog";
import { hostError, hostPorts, networkOfHost, portsText } from "./forwards";

const t = strings.security.forwards;

/** Which forward the dialog is open on, with the document as it opened. */
type Editing = { forward: PortForward | undefined; view: SiteView };

/**
 * Port forwards and the DMZ host (network.md, Firewall policy): each forward with its ports,
 * host and network; admins edit one at a time in a dialog, and the DMZ host inline. Each change
 * is the site's next revision.
 */
export function ForwardsCard(props: {
  view: SiteView;
  admin: boolean;
  hosts: readonly HostChoice[];
}) {
  const [editing, setEditing] = createSignal<Editing>();
  const forwards = () => props.view.config.firewall.forwards;
  const hostName = (ip: string) => props.hosts.find((h) => h.ip === ip)?.name;

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin}>
            <Button
              size="sm"
              icon={Plus}
              onClick={() => setEditing({ forward: undefined, view: props.view })}
            >
              {t.add}
            </Button>
          </Show>
        }
      />
      <Show
        when={forwards().length > 0}
        fallback={<p class="px-4 pb-4 text-fg-muted text-sm">{t.none}</p>}
      >
        <ul class="divide-y divide-edge border-edge border-y">
          <For each={forwards()}>
            {(forward) => (
              <li
                class="flex flex-wrap items-center gap-x-4 gap-y-1.5 px-4 py-3"
                aria-label={forward.name}
              >
                <div class="min-w-0 flex-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="font-medium text-sm">{forward.name}</span>
                    <Show when={!forward.enabled}>
                      <Badge>{t.off}</Badge>
                    </Show>
                    <Show when={forward.source}>
                      {(source) => <Badge>{t.onlyFrom(source())}</Badge>}
                    </Show>
                    <Show when={!forward.reflection}>
                      <Badge>{t.noReflection}</Badge>
                    </Show>
                  </div>
                  <p class="mt-1 flex flex-wrap items-center gap-1.5 text-fg-muted text-xs">
                    <span>{t.proto[forward.proto]}</span>
                    <span class="font-mono">{portsText(forward.ports)}</span>
                    <Icon icon={ArrowRight} class="size-3" />
                    <span class="font-mono">{t.to(forward.to, portsText(hostPorts(forward)))}</span>
                    <Show when={hostName(forward.to)}>{(name) => <span>· {name()}</span>}</Show>
                    <Show when={networkOfHost(props.view.config.networks, forward.to)}>
                      {(network) => <span>· {network().name}</span>}
                    </Show>
                  </p>
                </div>
                <Show when={props.admin}>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={t.edit(forward.name)}
                    onClick={() => setEditing({ forward, view: props.view })}
                  >
                    {strings.security.edit}
                  </Button>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Dmz view={props.view} admin={props.admin} hostName={hostName} />
      <Show when={editing()}>
        {(open) => (
          <ForwardDialog
            view={open().view}
            forward={open().forward}
            hosts={props.hosts}
            onClose={() => setEditing(undefined)}
          />
        )}
      </Show>
    </Card>
  );
}

/** The DMZ host: shown, set or removed inline. */
function Dmz(props: {
  view: SiteView;
  admin: boolean;
  hostName: (ip: string) => string | undefined;
}) {
  const [draft, setDraft] = createSignal<string>();
  const [error, setError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const host = () => props.view.config.firewall.dmz;

  const store = async (next: string | null) => {
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({ ...config, firewall: { ...config.firewall, dmz: next } }),
        t.dmzMessage(next),
      );
      setSaved(view.revision.id);
      setDraft(undefined);
    } catch (err) {
      const issue =
        err instanceof ApiError ? err.issues.find((i) => i.path === "firewall/dmz") : undefined;
      if (issue) setError(issue.message);
      else setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  const save = () => {
    const text = draft() ?? "";
    const why = hostError(props.view.config.networks, text);
    if (why) {
      setError(why);
      return;
    }
    void store(formatIpv4(ipv4(text) ?? 0));
  };

  return (
    <div class="flex flex-col gap-2 px-4 pt-3 pb-4">
      <div class="flex flex-wrap items-center gap-2">
        <h3 class="font-medium text-sm">{t.dmzTitle}</h3>
        <span class="flex-1" />
        <Show when={props.admin && draft() === undefined}>
          <Button
            size="sm"
            onClick={() => {
              setDraft(host() ?? "");
              setError(undefined);
              setSaved(undefined);
            }}
          >
            {strings.security.edit}
          </Button>
          <Show when={host()}>
            <Button
              size="sm"
              variant="ghost"
              icon={Trash2}
              aria-label={t.dmzRemove}
              disabled={busy()}
              onClick={() => void store(null)}
            />
          </Show>
        </Show>
      </div>
      <p class="text-fg-subtle text-xs">{t.dmzDescription}</p>
      <Show
        when={draft() !== undefined}
        fallback={
          <p class="text-sm">
            <Show when={host()} fallback={<span class="text-fg-muted">{t.dmzNone}</span>}>
              {(ip) => (
                <>
                  <span class="font-mono">{ip()}</span>
                  <Show when={props.hostName(ip())}>
                    {(name) => <span class="text-fg-muted"> · {name()}</span>}
                  </Show>
                </>
              )}
            </Show>
          </p>
        }
      >
        <form
          class="flex flex-wrap items-start gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            save();
          }}
        >
          <div class="min-w-48 flex-1">
            <input
              aria-label={t.dmzHost}
              placeholder={t.dmzHost}
              aria-invalid={error() ? "true" : undefined}
              class={[INPUT, "font-mono", error() ? "border-crit" : "border-edge"]}
              value={draft() ?? ""}
              onInput={(event) => {
                setDraft(event.currentTarget.value);
                setError(undefined);
              }}
            />
            <Show when={error()}>
              <p class="mt-1 text-crit text-xs">{error()}</p>
            </Show>
          </div>
          <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
            {strings.security.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {strings.security.save}
          </Button>
        </form>
      </Show>
      <RequestError text={failure()} />
      <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
    </div>
  );
}
