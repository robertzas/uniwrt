import { Plus, Trash2 } from "lucide";
import { createSignal, For, Match, Show, Switch } from "solid-js";
import type { ServiceFacts } from "../../../api/gen/ServiceFacts";
import type { SiteView } from "../../../api/gen/SiteView";
import type { Upstream } from "../../../api/gen/Upstream";
import { ApiError, errorText } from "../../../lib/api";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { Segmented } from "../../../ui/Segmented";
import { TextField } from "../../../ui/TextField";
import {
  dohResolvers,
  MAX,
  MODES,
  modeOf,
  PRESET_IDS,
  presetResolver,
  type ResolverDraft,
  resolverName,
  type UpstreamDraft,
  type UpstreamErrors,
  upstreamDraft,
  upstreamErrorsOfIssues,
  upstreamOf,
} from "./upstream";

const t = strings.networks.upstream;
const NO_ERRORS: UpstreamErrors = { servers: [], resolvers: [] };

/**
 * Upstream DNS (network.md, DHCP & local DNS; D-132): the ISP's servers, custom ones, or DNS
 * over HTTPS from presets, with the DoH proxies the gateway reports running. Admins edit it in
 * place and save it as one revision.
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
  const setServer = (index: number, value: string) =>
    change((now) => ({ servers: now.servers.map((s, i) => (i === index ? value : s)) }));
  const setResolver = (index: number, next: Partial<ResolverDraft>) =>
    change((now) => ({
      resolvers: now.resolvers.map((r, i) => (i === index ? { ...r, ...next } : r)),
    }));

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
        t.message(t.modes[now.mode] ?? now.mode),
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
            <Button
              size="sm"
              disabled={modeOf(upstream()) === undefined}
              title={modeOf(upstream()) === undefined ? t.layered : undefined}
              onClick={edit}
            >
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show when={draft()} fallback={<Summary upstream={upstream()} services={props.services} />}>
          {(now) => (
            <form
              class="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              <div class="flex flex-col gap-2">
                <Segmented
                  class="self-start"
                  label={t.modeLabel}
                  value={now().mode}
                  options={MODES.map((value) => ({ value, label: t.modes[value] ?? value }))}
                  onChange={(mode) => change(() => ({ mode }))}
                />
                <p class="text-fg-muted text-xs">{t.modeHints[now().mode]}</p>
              </div>
              <Show when={now().mode === "custom"}>
                <div class="grid gap-3 sm:grid-cols-2">
                  <For each={now().servers} keyed={false}>
                    {(server, i) => (
                      <div class="flex items-start gap-1">
                        <div class="min-w-0 flex-1">
                          <TextField
                            label={t.server(i + 1)}
                            value={server()}
                            error={errors().servers[i]}
                            onInput={(value) => setServer(i, value)}
                          />
                        </div>
                        <Button
                          class="mt-7"
                          variant="ghost"
                          icon={Trash2}
                          aria-label={t.removeServer(i + 1)}
                          onClick={() =>
                            change((d) => ({ servers: d.servers.filter((_, j) => j !== i) }))
                          }
                        />
                      </div>
                    )}
                  </For>
                </div>
                <Button
                  class="self-start"
                  size="sm"
                  icon={Plus}
                  disabled={now().servers.length >= MAX}
                  onClick={() => change((d) => ({ servers: [...d.servers, ""] }))}
                >
                  {t.addServer}
                </Button>
              </Show>
              <Show when={now().mode === "doh"}>
                <For each={now().resolvers} keyed={false}>
                  {(row, i) => (
                    <ResolverRow
                      row={row()}
                      index={i}
                      errors={errors().resolvers[i] ?? {}}
                      onChange={(next) => setResolver(i, next)}
                      onRemove={() =>
                        change((d) => ({ resolvers: d.resolvers.filter((_, j) => j !== i) }))
                      }
                    />
                  )}
                </For>
                <fieldset class="flex flex-wrap items-center gap-2" aria-label={t.addResolver}>
                  <span class="text-fg-muted text-xs">{t.addResolver}:</span>
                  <For each={[...PRESET_IDS, "custom" as const]}>
                    {(preset) => (
                      <Button
                        size="sm"
                        icon={Plus}
                        disabled={
                          now().resolvers.length >= MAX ||
                          (preset !== "custom" && now().resolvers.some((r) => r.preset === preset))
                        }
                        onClick={() =>
                          change((d) => ({ resolvers: [...d.resolvers, presetResolver(preset)] }))
                        }
                      >
                        {t.presets[preset]}
                      </Button>
                    )}
                  </For>
                </fieldset>
              </Show>
              <Show when={errors().list}>
                <p class="text-crit text-xs">{errors().list}</p>
              </Show>
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

function ResolverRow(props: {
  row: ResolverDraft;
  index: number;
  errors: { url?: string; bootstrap?: string };
  onChange: (next: Partial<ResolverDraft>) => void;
  onRemove: () => void;
}) {
  const name = () => t.presets[props.row.preset] ?? t.resolver(props.index + 1);
  return (
    <fieldset
      class="flex flex-col gap-3 rounded-control border border-edge p-3"
      aria-label={name()}
    >
      <div class="flex items-center gap-2">
        <p class="flex-1 font-medium text-sm">{name()}</p>
        <Button
          size="sm"
          variant="ghost"
          icon={Trash2}
          aria-label={t.removeResolver(name())}
          onClick={() => props.onRemove()}
        />
      </div>
      <div class="grid gap-3 sm:grid-cols-2">
        <Switch fallback={<UrlText url={props.row.url} error={props.errors.url} />}>
          <Match when={props.row.preset === "custom"}>
            <TextField
              label={t.url}
              value={props.row.url}
              hint={t.urlHint}
              error={props.errors.url}
              onInput={(url) => props.onChange({ url })}
            />
          </Match>
          <Match when={props.row.preset === "nextdns"}>
            <TextField
              label={t.profile}
              value={props.row.profile}
              hint={t.profileHint}
              error={props.errors.url}
              onInput={(profile) => props.onChange({ profile })}
            />
          </Match>
        </Switch>
        <TextField
          label={t.bootstrap}
          value={props.row.bootstrap}
          hint={t.bootstrapHint}
          error={props.errors.bootstrap}
          onInput={(bootstrap) => props.onChange({ bootstrap })}
        />
      </div>
    </fieldset>
  );
}

/** A preset's URL, which its row doesn't edit. */
function UrlText(props: { url: string; error: string | undefined }) {
  return (
    <div class="flex flex-col gap-1.5">
      <span class="font-medium text-sm">{t.url}</span>
      <span class="flex h-10 items-center truncate font-mono text-fg-muted text-xs">
        {props.url}
      </span>
      <Show when={props.error}>
        <p class="text-crit text-xs">{props.error}</p>
      </Show>
    </div>
  );
}

/** The upstream as saved, and for DoH, the proxies the gateway reports. */
function Summary(props: { upstream: Upstream; services: ServiceFacts | undefined }) {
  return (
    <Switch>
      <Match when={modeOf(props.upstream) === undefined}>
        <p class="text-fg-muted text-sm">{t.layered}</p>
      </Match>
      <Match when={modeOf(props.upstream) === "isp"}>
        <p class="text-fg-muted text-sm">{t.isp}</p>
      </Match>
      <Match when={servers(props.upstream)}>
        {(custom) => (
          <ul class="flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs" aria-label={t.modes.custom}>
            <For each={custom()}>{(server) => <li>{server}</li>}</For>
          </ul>
        )}
      </Match>
      <Match when={resolvers(props.upstream)}>
        {(doh) => (
          <>
            <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
              <For each={doh()}>
                {(resolver) => (
                  <>
                    <dt class="text-xs leading-5">{resolverName(resolver.url)}</dt>
                    <dd class="min-w-0 truncate font-mono text-fg-muted text-xs leading-5">
                      {resolver.url}
                      <Show when={resolver.bootstrap.length > 0}>
                        {` · ${t.bootstrapLabel} ${resolver.bootstrap.join(", ")}`}
                      </Show>
                    </dd>
                  </>
                )}
              </For>
            </dl>
            <Show when={props.services}>
              {(services) => <Proxies running={services().doh_instances} wanted={doh().length} />}
            </Show>
          </>
        )}
      </Match>
    </Switch>
  );
}

const servers = (upstream: Upstream) =>
  modeOf(upstream) === "custom" ? upstream.servers : undefined;
const resolvers = (upstream: Upstream) =>
  modeOf(upstream) === "doh" ? dohResolvers(upstream) : undefined;

function Proxies(props: { running: number | null; wanted: number }) {
  const running = () => props.running ?? 0;
  return (
    <p class="flex items-center gap-2 text-xs" role="status">
      <StatusDot tone={running() < props.wanted ? "warn" : "ok"} />
      {running() === 0 ? t.noProxy : t.proxies(running(), props.wanted)}
    </p>
  );
}
