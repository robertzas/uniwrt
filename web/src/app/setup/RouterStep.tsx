import { CircleCheck, Router, Search, TriangleAlert } from "lucide";
import { createEffect, createSignal, Match, onSettled, Show, Switch } from "solid-js";
import type { AdoptBody } from "../../api/gen/AdoptBody";
import type { Job } from "../../api/gen/Job";
import type { JobId } from "../../api/gen/JobId";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { parseAddress } from "../../features/devices/adopt/address";
import { api, errorText } from "../../lib/api";
import type { JobsState } from "../../lib/jobs";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { JobErrorNote, RequestError } from "../../ui/JobOutcome";
import { JobSteps } from "../../ui/JobSteps";
import { TextField } from "../../ui/TextField";

const t = strings.setup.router;

/** A router that could be the gateway: found at a usual address, or named by the user. */
interface Candidate {
  host: string;
  port: number;
  host_key: string;
  fingerprint: string;
  /** Root has no password; unknown for a router the user named. */
  blank: boolean | undefined;
  model: string | null;
  release: string | null;
  unsupported: string | null;
  adopted: number | null;
}

type Fate = "fresh" | "keep";

/**
 * The wizard's first step (adoption.md, Path E): the router at the controller's default gateway
 * or at OpenWrt's 192.168.1.1 is shown with its host key, adopted as the gateway and managed.
 * One that is configured already is asked about (D-082): start fresh, or keep its
 * configuration, which leaves the wizard for the import.
 */
export function RouterStep(props: {
  jobs: JobsState;
  addJobs: (jobs: readonly Job[]) => void;
  /** The adopted gateway, once there is one. */
  gateway: NodeSummary | undefined;
  onNext: () => void;
  /** The router was adopted to keep its configuration: on to the import. */
  onImport: () => void;
}) {
  const [searching, setSearching] = createSignal(false);
  const [looked, setLooked] = createSignal<string[]>([]);
  const [candidate, setCandidate] = createSignal<Candidate>();
  const [address, setAddress] = createSignal("");
  const [addressError, setAddressError] = createSignal<string>();
  const [password, setPassword] = createSignal("");
  const [fate, setFate] = createSignal<Fate>("fresh");
  const [error, setError] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [adoptId, setAdoptId] = createSignal<JobId>();
  const [manageId, setManageId] = createSignal<JobId>();

  const job = (id: JobId | undefined) => (id === undefined ? undefined : props.jobs.byId[id]);
  const adoption = () => job(adoptId());
  const managing = () => job(manageId());
  const working = () =>
    busy() || adoption()?.state === "running" || managing()?.state === "running";
  const configured = () => candidate()?.blank !== true;

  const search = async () => {
    setSearching(true);
    setError(undefined);
    try {
      const found = await api.bootstrap.gateways();
      setLooked(found.candidates);
      const first = found.gateways.find((g) => g.adopted === null) ?? found.gateways[0];
      setCandidate(first);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSearching(false);
    }
  };
  onSettled(() => {
    if (!props.gateway) void search();
  });

  const look = async () => {
    const target = parseAddress(address());
    if (!target) {
      setAddressError(t.addressInvalid);
      return;
    }
    setAddressError(undefined);
    setBusy(true);
    setError(undefined);
    try {
      const probe = await api.adopt.probe(target);
      setCandidate({
        ...probe,
        blank: undefined,
        model: null,
        release: null,
        unsupported: null,
        adopted: null,
      });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const manage = async (node: number) => {
    setBusy(true);
    setError(undefined);
    try {
      const started = await api.nodes.manage(node, true);
      props.addJobs([started]);
      setManageId(started.id);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const adopt = async (found: Candidate) => {
    setBusy(true);
    setError(undefined);
    setManageId(undefined);
    try {
      // The wizard says it is the gateway: before it has internet, nothing else tells.
      const body: AdoptBody = { host: found.host, host_key: found.host_key, role: "gateway" };
      if (found.port !== 22) body.port = found.port;
      body.password = found.blank ? "" : password();
      const started = await api.adopt.start(body);
      props.addJobs([started]);
      setAdoptId(started.id);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  // Once adopted: on to the import, or management, which the next steps need.
  createEffect(
    () => adoption()?.state,
    (state) => {
      const node = adoption()?.node;
      if (state !== "succeeded" || node === null || node === undefined) return;
      if (configured() && fate() === "keep") props.onImport();
      else if (manageId() === undefined) void manage(node);
    },
  );

  const progress = () => managing() ?? adoption();
  const failed = () => {
    const shown = progress();
    return shown?.state === "failed" ? shown.error : undefined;
  };

  return (
    <div class="flex flex-col gap-5">
      <h2 class="font-semibold text-lg tracking-tight">{t.heading}</h2>
      <Switch>
        <Match when={props.gateway?.mode === "managed" && props.gateway}>
          {(gateway) => (
            <Note
              tone="ok"
              title={t.ready(gateway().name)}
              text={gateway().status.state === "offline" ? t.offline : t.readyHint}
            />
          )}
        </Match>
        <Match when={progress()}>
          {(shown) => (
            <div class="flex flex-col gap-3">
              <p class="text-fg-muted text-sm">{managing() ? t.managing : t.adopting}</p>
              <div class="rounded-control border border-edge p-3" role="status" aria-live="polite">
                <JobSteps job={shown()} />
              </div>
              <Show when={failed()}>{(why) => <JobErrorNote error={why()} />}</Show>
            </div>
          )}
        </Match>
        <Match when={props.gateway}>
          {(gateway) => (
            <Note tone="info" title={t.observing(gateway().name)} text={t.observingHint} />
          )}
        </Match>
        <Match when={searching()}>
          <p class="flex items-center gap-2.5 text-fg-muted text-sm" role="status">
            <Icon icon={Search} class="size-4 animate-pulse" />
            {t.searching(looked().join(", ") || "192.168.1.1")}
          </p>
        </Match>
        <Match when={candidate()}>
          {(found) => (
            <div class="flex flex-col gap-4">
              <div class="flex items-start gap-3 rounded-control border border-edge p-4">
                <span class="grid size-10 shrink-0 place-items-center rounded-control bg-raised text-fg-muted">
                  <Icon icon={Router} class="size-5" />
                </span>
                <div class="min-w-0 flex-1">
                  <p class="flex flex-wrap items-center gap-2 font-medium">
                    {found().model ?? t.unnamed}
                    <Show when={found().blank !== undefined}>
                      <Badge tone={found().blank ? "ok" : "info"}>
                        {found().blank ? t.factory : t.configured}
                      </Badge>
                    </Show>
                  </p>
                  <p class="mt-0.5 text-fg-muted text-sm">
                    {[found().release, t.at(found().host)].filter(Boolean).join(" · ")}
                  </p>
                  <p class="mt-3 text-fg-subtle text-xs">{t.fingerprint}</p>
                  <p class="break-all font-mono text-sm">{found().fingerprint}</p>
                  <p class="mt-1 text-fg-subtle text-xs">{t.fingerprintHint}</p>
                </div>
              </div>
              <Switch>
                <Match when={found().unsupported}>
                  {(why) => <Note tone="warn" title={t.unsupported} text={why()} />}
                </Match>
                <Match when={found().adopted !== null}>
                  <Note tone="warn" title={t.adoptedElsewhere}>
                    <a href="/devices" class="text-accent text-sm hover:underline">
                      {t.openDevices}
                    </a>
                  </Note>
                </Match>
                <Match when={found().blank}>
                  <p class="text-fg-muted text-sm">{t.factoryHint}</p>
                </Match>
                <Match when={true}>
                  <div class="flex flex-col gap-4">
                    <TextField
                      label={t.password}
                      name="root-password"
                      type="password"
                      autocomplete="off"
                      value={password()}
                      onInput={setPassword}
                      hint={
                        found().blank === undefined
                          ? `${t.passwordHint} ${t.passwordMaybe}`
                          : t.passwordHint
                      }
                    />
                    <fieldset class="flex flex-col gap-2">
                      <legend class="mb-1 font-medium text-sm">{t.choice}</legend>
                      <Choice
                        name="fate"
                        checked={fate() === "fresh"}
                        onPick={() => setFate("fresh")}
                        label={t.fresh}
                        hint={t.freshHint}
                      />
                      <Choice
                        name="fate"
                        checked={fate() === "keep"}
                        onPick={() => setFate("keep")}
                        label={t.keep}
                        hint={t.keepHint}
                      />
                    </fieldset>
                  </div>
                </Match>
              </Switch>
            </div>
          )}
        </Match>
        <Match when={true}>
          <Note
            tone="warn"
            title={t.none}
            text={t.noneHint(looked().join(", ") || "192.168.1.1")}
          />
        </Match>
      </Switch>

      <Show when={!props.gateway && !progress() && !searching()}>
        <details open={!candidate()} class="rounded-control border border-edge">
          <summary class="cursor-pointer select-none px-3 py-2 font-medium text-sm">
            {t.elsewhere}
          </summary>
          <form
            class="flex flex-col gap-3 border-edge border-t p-3"
            onSubmit={(event) => {
              event.preventDefault();
              void look();
            }}
          >
            <TextField
              label={t.address}
              name="router-address"
              value={address()}
              onInput={setAddress}
              hint={t.addressHint}
              error={addressError()}
            />
            <div class="flex flex-wrap gap-2">
              <Button type="submit" disabled={working() || address().trim() === ""}>
                {t.look}
              </Button>
              <Button variant="ghost" disabled={working()} onClick={() => void search()}>
                {t.again}
              </Button>
            </div>
          </form>
        </details>
      </Show>

      <RequestError text={error()} />

      <div class="flex flex-wrap justify-end gap-2">
        <Switch>
          <Match when={props.gateway?.mode === "managed"}>
            <Button variant="primary" onClick={props.onNext}>
              {strings.setup.next}
            </Button>
          </Match>
          <Match when={failed() && candidate()}>
            {(found) => (
              <Button variant="primary" disabled={working()} onClick={() => void adopt(found())}>
                {strings.setup.retry}
              </Button>
            )}
          </Match>
          <Match when={progress()}>
            <Button disabled>{strings.setup.next}</Button>
          </Match>
          <Match when={props.gateway}>
            {(gateway) => (
              <Button
                variant="primary"
                disabled={working()}
                onClick={() => void manage(gateway().id)}
              >
                {t.manage}
              </Button>
            )}
          </Match>
          <Match when={candidate()}>
            {(found) => (
              <Button
                variant="primary"
                disabled={
                  working() ||
                  searching() ||
                  found().unsupported !== null ||
                  found().adopted !== null ||
                  (found().blank === false && password() === "")
                }
                onClick={() => void adopt(found())}
              >
                {found().blank ? t.use : fate() === "keep" ? t.adoptKeep : t.adopt}
              </Button>
            )}
          </Match>
        </Switch>
      </div>
    </div>
  );
}

const TONES = {
  ok: { box: "border-ok/40 bg-ok/10", icon: "text-ok" },
  info: { box: "border-edge", icon: "text-fg-muted" },
  warn: { box: "border-warn/40 bg-warn/10", icon: "text-warn" },
} as const;

/** A titled note with its tone's icon. */
export function Note(props: {
  tone: keyof typeof TONES;
  title: string;
  text?: string;
  children?: import("@solidjs/web").JSX.Element;
}) {
  return (
    <div
      class={[
        "flex items-start gap-2.5 rounded-control border px-3 py-2.5 text-sm",
        TONES[props.tone].box,
      ]}
    >
      <Icon
        icon={props.tone === "ok" ? CircleCheck : props.tone === "warn" ? TriangleAlert : Router}
        class={["mt-0.5 size-4 shrink-0", TONES[props.tone].icon]}
      />
      <div class="min-w-0">
        <p class="font-medium text-fg">{props.title}</p>
        <Show when={props.text}>
          <p class="mt-0.5 text-fg-muted">{props.text}</p>
        </Show>
        {props.children}
      </div>
    </div>
  );
}

/** One of a group's exclusive choices, with what it means. */
function Choice(props: {
  name: string;
  checked: boolean;
  onPick: () => void;
  label: string;
  hint: string;
}) {
  return (
    <label class="flex cursor-pointer items-start gap-2.5 rounded-control border border-edge px-3 py-2.5 has-[:checked]:border-accent">
      <input
        type="radio"
        name={props.name}
        checked={props.checked}
        onChange={() => props.onPick()}
        class="mt-1 accent-(--color-accent-fill)"
      />
      <span class="min-w-0">
        <span class="block font-medium text-sm">{props.label}</span>
        <span class="mt-0.5 block text-fg-muted text-sm">{props.hint}</span>
      </span>
    </label>
  );
}
