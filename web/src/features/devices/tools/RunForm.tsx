import { Play } from "lucide";
import {
  createMemo,
  createSignal,
  createUniqueId,
  For,
  Match,
  Show,
  Switch,
  useContext,
} from "solid-js";
import type { DiagRun } from "../../../api/gen/DiagRun";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { OptionalToolStatus } from "../../../api/gen/OptionalToolStatus";
import { api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { FormField, INPUT, SELECT } from "../../../ui/FormField";
import { Segmented } from "../../../ui/Segmented";
import {
  cablePorts,
  type Draft,
  draftErrors,
  emptyDraft,
  MAX_IPERF3_SECONDS,
  MAX_IPERF3_STREAMS,
  MAX_PING_COUNT,
  needs,
  RECORD_TYPES,
  TOOL_KINDS,
  toolOf,
} from "./draft";

const t = strings.device.tools;
const f = t.fields;

/**
 * Starts a run on the device (admins): ping, traceroute, a DNS lookup, an HTTP check, a
 * throughput test against another device or the controller, or a cable test. Each field is
 * checked as the model checks it, so a refusal is the device's, not the form's.
 */
export function RunForm(props: {
  node: Node;
  facts: NodeFacts | null;
  admin: boolean;
  online: boolean;
  tools: readonly OptionalToolStatus[] | undefined;
  onStarted: (run: DiagRun) => void;
}) {
  const live = useContext(LiveContext);
  const id = createUniqueId();
  const [draft, setDraft] = createSignal<Draft>(emptyDraft());
  const [touched, setTouched] = createSignal<ReadonlySet<keyof Draft>>(new Set());
  const [tried, setTried] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<string>();
  const ports = createMemo(() => cablePorts(props.facts?.board.layout));
  /** The draft with the first port picked until another is. */
  const current = createMemo(() => {
    const now = draft();
    return now.port ? now : { ...now, port: ports()[0] ?? "" };
  });
  const errors = createMemo(() => draftErrors(current()));
  const error = (field: keyof Draft) =>
    tried() || touched().has(field) ? errors()[field] : undefined;
  const update = (patch: Partial<Draft>) => {
    setDraft((now) => ({ ...now, ...patch }));
    setFailure(undefined);
  };
  const touch = (field: keyof Draft) => setTouched((now) => new Set([...now, field]));
  const peers = createMemo(() => live.fleet.nodes.filter((node) => node.id !== props.node.id));
  const missing = () => {
    const tool = needs(current().tool);
    const status = tool && props.tools?.find((entry) => entry.tool === tool);
    return status && !status.installed ? status.package : undefined;
  };
  const blocked = () =>
    !props.admin ? t.form.adminOnly : !props.online ? t.form.offline : undefined;

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    setTried(true);
    const tool = toolOf(current());
    if (!tool || busy() || blocked()) return;
    setBusy(true);
    setFailure(undefined);
    try {
      props.onStarted(await api.diag.start({ node: props.node.id, tool }));
    } catch (failed) {
      setFailure(errorText(failed));
    } finally {
      setBusy(false);
    }
  };

  const text = (field: "target" | "name" | "server" | "url", label: string, hint?: string) => (
    <FormField label={label} for={`${id}-${field}`} hint={hint} error={error(field)}>
      <input
        id={`${id}-${field}`}
        class={[INPUT, error(field) ? "border-crit" : "border-edge"]}
        value={current()[field]}
        autocapitalize="none"
        spellcheck={false}
        aria-invalid={error(field) ? "true" : undefined}
        onInput={(event) => update({ [field]: event.currentTarget.value })}
        onBlur={() => touch(field)}
      />
    </FormField>
  );
  const number = (field: "count" | "seconds" | "streams", label: string, max: number) => (
    <FormField label={label} for={`${id}-${field}`} error={error(field)}>
      <input
        id={`${id}-${field}`}
        type="number"
        min="1"
        max={max}
        class={[INPUT, error(field) ? "border-crit" : "border-edge"]}
        value={current()[field]}
        aria-invalid={error(field) ? "true" : undefined}
        onInput={(event) => update({ [field]: event.currentTarget.value })}
        onBlur={() => touch(field)}
      />
    </FormField>
  );
  const family = () => (
    <FormField label={f.family} for={`${id}-family`}>
      <select
        id={`${id}-family`}
        class={SELECT}
        value={current().family}
        onChange={(event) => update({ family: event.currentTarget.value as Draft["family"] })}
      >
        <option value="">{f.familyAny}</option>
        <option value="ipv4">{f.ipv4}</option>
        <option value="ipv6">{f.ipv6}</option>
      </select>
    </FormField>
  );

  return (
    <Card aria-label={t.form.title}>
      <CardHeader title={t.form.title} description={t.form.description} />
      <form class="flex flex-col gap-4 px-4 pb-4" onSubmit={submit} novalidate>
        <fieldset disabled={!props.admin} class="flex min-w-0 flex-col gap-4">
          <Segmented
            label={t.form.tool}
            value={current().tool}
            options={TOOL_KINDS.map((tool) => ({
              value: tool,
              label: t.tool[tool],
              description: t.toolLong[tool],
            }))}
            onChange={(tool) => {
              update({ tool });
              setTried(false);
            }}
            class="flex-wrap self-start"
          />
          <Switch>
            <Match when={current().tool === "ping"}>
              {text("target", f.target, f.targetHint)}
              <div class="grid grid-cols-2 gap-3">
                {number("count", f.count, MAX_PING_COUNT)}
                {family()}
              </div>
            </Match>
            <Match when={current().tool === "traceroute"}>
              {text("target", f.target, f.targetHint)}
              {family()}
            </Match>
            <Match when={current().tool === "dns"}>
              {text("name", f.name, f.nameHint)}
              <div class="grid grid-cols-2 gap-3">
                <FormField label={f.record} for={`${id}-record`}>
                  <select
                    id={`${id}-record`}
                    class={SELECT}
                    value={current().record}
                    onChange={(event) =>
                      update({ record: event.currentTarget.value as Draft["record"] })
                    }
                  >
                    <option value="">{f.recordBoth}</option>
                    <For each={RECORD_TYPES}>{(type) => <option value={type}>{type}</option>}</For>
                  </select>
                </FormField>
                {text("server", f.server, f.serverHint)}
              </div>
            </Match>
            <Match when={current().tool === "http"}>{text("url", f.url, f.urlHint)}</Match>
            <Match when={current().tool === "iperf3"}>
              <FormField label={f.peer} for={`${id}-peer`} error={error("peer")}>
                <select
                  id={`${id}-peer`}
                  class={SELECT}
                  value={current().peer}
                  onChange={(event) => update({ peer: event.currentTarget.value })}
                >
                  <option value="controller">{f.controller}</option>
                  <For each={peers()}>
                    {(peer) => (
                      <option value={String(peer.id)} disabled={peer.status.state === "offline"}>
                        {peer.status.state === "offline" ? f.offlinePeer(peer.name) : peer.name}
                      </option>
                    )}
                  </For>
                </select>
              </FormField>
              <div class="flex flex-col gap-1.5">
                <span class="font-medium text-fg text-sm">{f.direction}</span>
                <Segmented
                  label={f.direction}
                  value={current().direction}
                  options={(["send", "receive", "both"] as const).map((direction) => ({
                    value: direction,
                    label: t.direction[direction].label,
                    description: t.direction[direction].description,
                  }))}
                  onChange={(direction) => update({ direction })}
                  class="self-start"
                />
              </div>
              <div class="grid grid-cols-2 gap-3">
                {number("seconds", f.seconds, MAX_IPERF3_SECONDS)}
                {number("streams", f.streams, MAX_IPERF3_STREAMS)}
              </div>
            </Match>
            <Match when={current().tool === "cable_test"}>
              <Show
                when={ports().length > 0}
                fallback={<p class="text-fg-subtle text-sm">{t.form.noPorts}</p>}
              >
                <FormField
                  label={f.port}
                  for={`${id}-port`}
                  hint={t.form.cableWarning}
                  error={error("port")}
                >
                  <select
                    id={`${id}-port`}
                    class={SELECT}
                    value={current().port}
                    onChange={(event) => update({ port: event.currentTarget.value })}
                  >
                    <For each={ports()}>{(port) => <option value={port}>{port}</option>}</For>
                  </select>
                </FormField>
              </Show>
            </Match>
          </Switch>
        </fieldset>
        <Show when={missing()}>
          {(pkg) => <p class="text-sm text-warn">{t.form.missing(pkg())}</p>}
        </Show>
        <Show when={failure()}>
          {(message) => (
            <p role="alert" class="text-crit text-sm">
              {message()}
            </p>
          )}
        </Show>
        <div class="flex flex-wrap items-center justify-end gap-3">
          <Show when={blocked()}>
            {(why) => <p class="mr-auto text-fg-subtle text-xs">{why()}</p>}
          </Show>
          <Button
            type="submit"
            variant="primary"
            icon={Play}
            disabled={busy() || blocked() !== undefined}
          >
            {busy() ? t.form.starting : t.form.run}
          </Button>
        </div>
      </form>
    </Card>
  );
}
