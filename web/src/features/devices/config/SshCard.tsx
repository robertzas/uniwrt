import { useQuery } from "@tanstack/solid-query";
import { createSignal, Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { SiteView } from "../../../api/gen/SiteView";
import type { SshHardening } from "../../../api/gen/SshHardening";
import { api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { managedOf } from "./packages";
import { managementNetwork, sshNow, sshOf, withSsh } from "./ssh";

const t = strings.device.ssh;

/**
 * A device's SSH hardening (security.md, Node hardening; D-150), opt-in: keys only, and the
 * server on the management network alone; with how its servers run now. Saved, it is applied
 * like any change, whose verify logs in anew before it confirms.
 */
export function SshCard(props: { node: Node; facts: NodeFacts | null; admin: boolean }) {
  const live = useContext(LiveContext);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Show
        when={site.data}
        fallback={
          <div class="px-4 pb-4">
            <RequestError text={site.error?.message} />
          </div>
        }
      >
        {(view) => (
          <SshForm view={view()} node={props.node} facts={props.facts} admin={props.admin} />
        )}
      </Show>
    </Card>
  );
}

function SshForm(props: { view: SiteView; node: Node; facts: NodeFacts | null; admin: boolean }) {
  const stored = () => sshOf(props.view.config, props.node.id);
  const [edits, setEdits] = createSignal<Partial<SshHardening>>({});
  const draft = (): SshHardening => ({ ...stored(), ...edits() });
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const managed = () => managedOf(props.view.config, props.node.id);
  const network = () => managementNetwork(props.view.config)?.name ?? "lan";
  const canBind = () => managed().includes("network");
  const dirty = () => JSON.stringify(draft()) !== JSON.stringify(stored());
  const now = () => sshNow(props.facts?.services.ssh);
  const change = (next: Partial<SshHardening>) => {
    setEdits((was) => ({ ...was, ...next }));
    setSaved(undefined);
  };
  const save = async () => {
    const ssh = draft();
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => withSsh(config, props.node.id, ssh),
        t.message(props.node.name),
      );
      setEdits({});
      setSaved(view.revision.id);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  const bindHint = () => {
    if (!canBind()) return t.needsNetwork;
    const hint = t.managementOnlyHint(network());
    return props.node.role === "gateway" ? `${hint} ${t.gatewayHint(network())}` : hint;
  };

  return (
    <form
      class="flex flex-col gap-3 px-4 pb-4"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <Show when={props.node.mode !== "managed"}>
        <p class="text-fg-muted text-sm">{t.observed}</p>
      </Show>
      <Checkbox
        checked={draft().key_only}
        onChange={(key_only) => change({ key_only })}
        label={t.keyOnly}
        hint={t.keyOnlyHint}
        disabled={!props.admin}
      />
      <Checkbox
        checked={draft().management_only}
        onChange={(management_only) => change({ management_only })}
        label={t.managementOnly(network())}
        hint={bindHint()}
        disabled={!props.admin || (!canBind() && !draft().management_only)}
      />
      <Show
        when={(stored().key_only || stored().management_only) && !managed().includes("dropbear")}
      >
        <p class="text-warn text-xs">{t.unmanaged}</p>
      </Show>
      <Show when={now()}>
        {(state) => (
          <p class="text-fg-muted text-xs">{t.now(state().passwords, state().devices)}</p>
        )}
      </Show>
      <RequestError text={failure()} />
      <Show when={saved()}>
        {(revision) => <SavedRevision revision={revision()} note={t.saved} />}
      </Show>
      <Show when={props.admin && dirty()}>
        <div class="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => setEdits({})} disabled={busy()}>
            {t.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {t.save}
          </Button>
        </div>
      </Show>
    </form>
  );
}
