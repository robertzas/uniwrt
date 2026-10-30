import type { JSX } from "@solidjs/web";
import { Fingerprint, Terminal } from "lucide";
import { createSignal, For, Match, Show, Switch, untrack } from "solid-js";
import type { AdoptBody } from "../../../api/gen/AdoptBody";
import type { Job } from "../../../api/gen/Job";
import type { JobErrorCode } from "../../../api/gen/JobErrorCode";
import type { JobId } from "../../../api/gen/JobId";
import type { NodeId } from "../../../api/gen/NodeId";
import type { NodeRole } from "../../../api/gen/NodeRole";
import type { ProbeResult } from "../../../api/gen/ProbeResult";
import { ApiError, api } from "../../../lib/api";
import type { JobsState } from "../../../lib/jobs";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { TextField } from "../../../ui/TextField";
import { formatAddress, parseAddress } from "./address";
import { JobErrorNote, JobSuccessNote, RequestError } from "./JobOutcome";
import { JobSteps } from "./JobSteps";

const s = strings.adopt;

export interface AdoptDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The address to start with, such as a discovered device's. */
  host?: string | undefined;
  jobs: JobsState;
  /** Adds the job a request answered with, before the `jobs` topic reports it. */
  addJobs: (jobs: readonly Job[]) => void;
  onOpenDevice: (id: NodeId) => void;
}

/** The adoption wizard: the address and password, the fingerprint to confirm, the job live. */
export function AdoptDialog(props: AdoptDialogProps) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={s.title}
      initialFocus={() =>
        document.querySelector<HTMLElement>("[data-autofocus] input") ?? undefined
      }
    >
      <Wizard {...props} />
    </Dialog>
  );
}

type Stage =
  | { kind: "address" }
  | { kind: "confirm"; probe: ProbeResult }
  | { kind: "progress"; probe: ProbeResult; job: JobId };

function requestError(error: unknown): string {
  return error instanceof ApiError ? error.message : strings.auth.unreachable.title;
}

/** The dialog's body; Kobalte mounts it on open, so every adoption starts afresh. */
function Wizard(props: AdoptDialogProps) {
  const [stage, setStage] = createSignal<Stage>({ kind: "address" });
  const [address, setAddress] = createSignal(untrack(() => props.host ?? ""));
  const [addressError, setAddressError] = createSignal<string>();
  const [password, setPassword] = createSignal("");
  const [remember, setRemember] = createSignal(false);
  const [name, setName] = createSignal("");
  const [role, setRole] = createSignal<NodeRole>();
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();

  const run = async (work: () => Promise<void>) => {
    if (pending()) return;
    setPending(true);
    setError(undefined);
    try {
      await work();
    } catch (e) {
      setError(requestError(e));
    } finally {
      setPending(false);
    }
  };

  const readKey = () =>
    run(async () => {
      const target = parseAddress(address());
      if (!target) {
        setAddressError(s.addressInvalid);
        return;
      }
      setAddressError(undefined);
      setStage({ kind: "confirm", probe: await api.adopt.probe(target) });
    });

  const adopt = (probe: ProbeResult) =>
    run(async () => {
      const body: AdoptBody = { host: probe.host, host_key: probe.host_key };
      if (probe.port !== 22) body.port = probe.port;
      if (password()) {
        body.password = password();
        if (remember()) body.remember_password = true;
      }
      const display = name().trim();
      if (display) body.name = display;
      const chosen = role();
      if (chosen) body.role = chosen;
      const job = await api.adopt.start(body);
      props.addJobs([job]);
      setStage({ kind: "progress", probe, job: job.id });
    });

  /** What a failure's action does: go back to the step whose input it concerns, or retry. */
  const recover = (code: JobErrorCode, probed: ProbeResult) => {
    switch (code) {
      case "auth_rejected":
        setPassword("");
        setStage({ kind: "address" });
        break;
      case "host_key_mismatch":
        setStage({ kind: "address" });
        void readKey();
        break;
      case "already_adopted":
        props.onOpenChange(false);
        break;
      case "unreachable":
      case "unsupported":
        setStage({ kind: "address" });
        break;
      default:
        void adopt(probed);
    }
  };

  const confirming = () => {
    const current = stage();
    return current.kind === "confirm" ? current : undefined;
  };
  const progressing = () => {
    const current = stage();
    return current.kind === "progress" ? current : undefined;
  };

  const reset = () => {
    setAddress("");
    setPassword("");
    setRemember(false);
    setName("");
    setRole(undefined);
    setError(undefined);
    setStage({ kind: "address" });
  };

  return (
    <div class="px-5 pt-1 pb-5">
      <Switch>
        <Match when={stage().kind === "address"}>
          <form
            class="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              void readKey();
            }}
          >
            <p class="text-fg-muted text-sm">{s.description}</p>
            <RequestError text={error()} />
            <div data-autofocus>
              <TextField
                label={s.address}
                name="address"
                value={address()}
                onInput={setAddress}
                hint={s.addressHint}
                error={addressError()}
                autocomplete="off"
                required
              />
            </div>
            <TextField
              label={s.password}
              name="password"
              type="password"
              autocomplete="off"
              value={password()}
              onInput={(value) => {
                setPassword(value);
                if (!value) setRemember(false);
              }}
              hint={s.passwordHint}
            />
            <Checkbox
              label={s.remember}
              hint={s.rememberHint}
              checked={remember()}
              onChange={setRemember}
              disabled={!password()}
            />
            <Footer>
              <Button onClick={() => props.onOpenChange(false)}>{s.cancel}</Button>
              <Button type="submit" variant="primary" disabled={pending()}>
                {pending() ? s.probing : s.next}
              </Button>
            </Footer>
          </form>
        </Match>
        <Match when={confirming()}>
          {(confirm) => (
            <Confirm
              probe={confirm().probe}
              name={name()}
              onName={setName}
              role={role()}
              onRole={setRole}
              error={error()}
              pending={pending()}
              onBack={() => {
                setError(undefined);
                setStage({ kind: "address" });
              }}
              onAdopt={(probe) => void adopt(probe)}
            />
          )}
        </Match>
        <Match when={progressing() && props.jobs.byId[progressing()?.job ?? -1]}>
          {(job) => (
            <Progress
              job={job()}
              address={formatAddress(progressing()?.probe.host ?? "", progressing()?.probe.port)}
              pending={pending()}
              error={error()}
              onRecover={(code) => {
                const current = progressing();
                if (current) recover(code, current.probe);
              }}
              onOpenDevice={props.onOpenDevice}
              onAnother={reset}
              onClose={() => props.onOpenChange(false)}
            />
          )}
        </Match>
      </Switch>
    </div>
  );
}

function Footer(props: { children: JSX.Element }) {
  return <div class="mt-2 flex flex-wrap justify-end gap-2">{props.children}</div>;
}

const roles: { value: NodeRole | undefined; label: string }[] = [
  { value: undefined, label: s.confirm.roleAuto },
  { value: "gateway", label: strings.devices.role.gateway },
  { value: "ap", label: strings.devices.role.ap },
];

function Confirm(props: {
  probe: ProbeResult;
  name: string;
  onName: (name: string) => void;
  role: NodeRole | undefined;
  onRole: (role: NodeRole | undefined) => void;
  error: string | undefined;
  pending: boolean;
  onBack: () => void;
  onAdopt: (probe: ProbeResult) => void;
}) {
  return (
    <form
      class="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault();
        props.onAdopt(props.probe);
      }}
    >
      <div>
        <h3 class="font-medium text-sm">{s.confirm.title}</h3>
        <p class="mt-1 text-fg-muted text-sm">{s.confirm.description}</p>
      </div>
      <RequestError text={props.error} />
      <div class="rounded-control border border-accent/40 bg-accent/5 px-3.5 py-3">
        <p class="flex items-center gap-1.5 text-fg-muted text-xs">
          <Icon icon={Fingerprint} class="size-3.5 text-accent" />
          {s.confirm.fingerprint(props.probe.algorithm)} ·{" "}
          <span class="font-mono">{formatAddress(props.probe.host, props.probe.port)}</span>
        </p>
        <p class="mt-1.5 break-all font-medium font-mono text-fg text-sm" data-testid="fingerprint">
          {props.probe.fingerprint}
        </p>
      </div>
      <div class="text-fg-muted text-xs">
        <p>{s.confirm.howTo}</p>
        <p class="mt-1 flex items-center gap-1.5 overflow-x-auto rounded-control bg-raised px-2.5 py-1.5 font-mono text-fg">
          <Icon icon={Terminal} class="size-3.5 shrink-0 text-fg-subtle" />
          <span class="whitespace-nowrap">{s.confirm.command}</span>
        </p>
      </div>
      <TextField
        label={s.confirm.name}
        name="name"
        value={props.name}
        onInput={props.onName}
        hint={s.confirm.nameHint}
        autocomplete="off"
      />
      <fieldset>
        <legend class="font-medium text-fg text-sm">{s.confirm.role}</legend>
        <div class="mt-1.5 inline-flex rounded-control border border-edge bg-canvas p-0.5">
          <For each={roles}>
            {(option) => (
              <label
                class={[
                  "cursor-pointer rounded-[calc(var(--radius-control)-2px)] px-3 py-1 text-sm has-focus-visible:ring-2 has-focus-visible:ring-accent/40",
                  props.role === option.value
                    ? "bg-surface font-medium text-fg shadow-sm"
                    : "text-fg-muted hover:text-fg",
                ]}
              >
                <input
                  type="radio"
                  name="role"
                  class="sr-only"
                  checked={props.role === option.value}
                  onChange={() => props.onRole(option.value)}
                />
                {option.label}
              </label>
            )}
          </For>
        </div>
        <Show when={props.role === undefined}>
          <p class="mt-1.5 text-fg-subtle text-xs">{s.confirm.roleAutoHint}</p>
        </Show>
      </fieldset>
      <Footer>
        <Button onClick={() => props.onBack()}>{s.back}</Button>
        <Button type="submit" variant="primary" disabled={props.pending}>
          {props.pending ? s.confirm.starting : s.confirm.submit}
        </Button>
      </Footer>
    </form>
  );
}

function Progress(props: {
  job: Job;
  address: string;
  pending: boolean;
  error: string | undefined;
  onRecover: (code: JobErrorCode) => void;
  onOpenDevice: (id: NodeId) => void;
  onAnother: () => void;
  onClose: () => void;
}) {
  return (
    <div class="flex flex-col gap-4">
      <h3 class="font-medium text-sm">{s.progress.title(props.address)}</h3>
      <JobSteps job={props.job} />
      <RequestError text={props.error} />
      <Switch>
        <Match when={props.job.state === "running"}>
          <p class="text-fg-subtle text-xs">{s.progress.running}</p>
          <Footer>
            <Button onClick={() => props.onClose()}>{s.progress.close}</Button>
          </Footer>
        </Match>
        <Match when={props.job.state === "succeeded"}>
          <JobSuccessNote text={s.progress.succeeded} />
          <Footer>
            <Button onClick={() => props.onAnother()}>{s.progress.another}</Button>
            <Show when={props.job.node}>
              {(node) => (
                <Button variant="primary" onClick={() => props.onOpenDevice(node())}>
                  {s.progress.open}
                </Button>
              )}
            </Show>
          </Footer>
        </Match>
        <Match when={props.job.error}>
          {(error) => (
            <>
              <JobErrorNote error={error()} />
              <Footer>
                <Button onClick={() => props.onClose()}>{s.progress.close}</Button>
                <Show when={error().code !== "already_adopted"}>
                  <Button
                    variant="primary"
                    disabled={props.pending}
                    onClick={() => props.onRecover(error().code)}
                  >
                    {s.error[error().code].action}
                  </Button>
                </Show>
              </Footer>
            </>
          )}
        </Match>
      </Switch>
    </div>
  );
}
