import type { JSX } from "@solidjs/web";
import { createSignal, Match, Switch, useContext } from "solid-js";
import type { JobErrorCode } from "../../../api/gen/JobErrorCode";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import type { ProbeResult } from "../../../api/gen/ProbeResult";
import type { ReplaceBody } from "../../../api/gen/ReplaceBody";
import { api, errorText } from "../../../lib/api";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { JobErrorNote, JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";
import { TextField } from "../../../ui/TextField";
import { parseAddress } from "../adopt/address";
import { HostKey } from "../adopt/HostKey";

const a = strings.adopt;
const l = strings.device.lifecycle;
const r = l.replace;

/**
 * Replaces a node's device (D-131) as the adoption wizard adopts one: the new device's address
 * and password, its probed fingerprint to confirm, then the job, whose Record step lists the
 * settings that found no match on it. The moved settings are a revision, reviewed before an apply.
 */
export function ReplaceDialog(props: { node: Node; open: boolean; onClose: () => void }) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={r.title(props.node.name)}
      initialFocus={() =>
        document.querySelector<HTMLElement>("[data-autofocus] input") ?? undefined
      }
    >
      <Wizard node={props.node} onClose={props.onClose} />
    </Dialog>
  );
}

type Stage =
  | { kind: "address" }
  | { kind: "confirm"; probe: ProbeResult }
  | { kind: "progress"; probe: ProbeResult; job: JobId };

function Wizard(props: { node: Node; onClose: () => void }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [stage, setStage] = createSignal<Stage>({ kind: "address" });
  const [address, setAddress] = createSignal("");
  const [addressError, setAddressError] = createSignal<string>();
  const [password, setPassword] = createSignal("");
  const [remember, setRemember] = createSignal(false);
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();

  const run = async (work: () => Promise<void>) => {
    if (pending()) return;
    setPending(true);
    setError(undefined);
    try {
      await work();
    } catch (err) {
      setError(errorText(err) ?? strings.auth.unreachable.title);
    } finally {
      setPending(false);
    }
  };

  const readKey = () =>
    run(async () => {
      const target = parseAddress(address());
      if (!target) {
        setAddressError(a.addressInvalid);
        return;
      }
      setAddressError(undefined);
      setStage({ kind: "confirm", probe: await api.adopt.probe(target) });
    });

  const replace = (probe: ProbeResult) =>
    run(async () => {
      const body: ReplaceBody = { host: probe.host, host_key: probe.host_key };
      if (probe.port !== 22) body.port = probe.port;
      if (password()) {
        body.password = password();
        if (remember()) body.remember_password = true;
      }
      const job = await api.nodes.replace(props.node.id, body);
      addJobs([job]);
      setStage({ kind: "progress", probe, job: job.id });
    });

  /** What a failure's action does: back to the input it concerns, or try again. */
  const recover = (code: JobErrorCode, probe: ProbeResult) => {
    switch (code) {
      case "auth_rejected":
        setPassword("");
        setStage({ kind: "address" });
        break;
      case "host_key_mismatch":
        setStage({ kind: "address" });
        void readKey();
        break;
      case "unreachable":
      case "unsupported":
        setStage({ kind: "address" });
        break;
      case "already_adopted":
      case "identity_mismatch":
        props.onClose();
        break;
      default:
        void replace(probe);
    }
  };

  const confirming = () => {
    const current = stage();
    return current.kind === "confirm" ? current.probe : undefined;
  };
  const progressing = () => {
    const current = stage();
    return current.kind === "progress" ? current : undefined;
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
            <p class="text-fg-muted text-sm">{r.description}</p>
            <RequestError text={error()} />
            <div data-autofocus>
              <TextField
                label={a.address}
                name="address"
                value={address()}
                onInput={setAddress}
                hint={a.addressHint}
                error={addressError()}
                autocomplete="off"
                required
              />
            </div>
            <TextField
              label={a.password}
              name="password"
              type="password"
              autocomplete="off"
              value={password()}
              onInput={(value) => {
                setPassword(value);
                if (!value) setRemember(false);
              }}
              hint={a.passwordHint}
            />
            <Checkbox
              label={a.remember}
              hint={a.rememberHint}
              checked={remember()}
              onChange={setRemember}
              disabled={!password()}
            />
            <Footer>
              <Button variant="ghost" onClick={() => props.onClose()}>
                {l.cancel}
              </Button>
              <Button type="submit" variant="primary" disabled={pending()}>
                {pending() ? a.probing : a.next}
              </Button>
            </Footer>
          </form>
        </Match>
        <Match when={confirming()}>
          {(probe) => (
            <form
              class="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                void replace(probe());
              }}
            >
              <div>
                <h3 class="font-medium text-sm">{a.confirm.title}</h3>
                <p class="mt-1 text-fg-muted text-sm">{a.confirm.description}</p>
              </div>
              <RequestError text={error()} />
              <HostKey probe={probe()} />
              <Footer>
                <Button
                  onClick={() => {
                    setError(undefined);
                    setStage({ kind: "address" });
                  }}
                >
                  {a.back}
                </Button>
                <Button type="submit" variant="primary" disabled={pending()}>
                  {pending() ? a.confirm.starting : r.submit}
                </Button>
              </Footer>
            </form>
          )}
        </Match>
        <Match when={progressing() && jobs.byId[progressing()?.job ?? -1]}>
          {(job) => (
            <div class="flex flex-col gap-4">
              <h3 class="font-medium text-sm">{r.progress(props.node.name)}</h3>
              <JobSteps job={job()} />
              <RequestError text={error()} />
              <Switch>
                <Match when={job().state === "running"}>
                  <p class="text-fg-subtle text-xs">{r.running}</p>
                  <Footer>
                    <Button onClick={() => props.onClose()}>{l.close}</Button>
                  </Footer>
                </Match>
                <Match when={job().state === "succeeded"}>
                  <JobSuccessNote text={r.succeeded} />
                  <Footer>
                    <Button onClick={() => props.onClose()}>{l.close}</Button>
                    <a
                      href="/changes"
                      class="inline-flex h-8 items-center rounded-control bg-accent-fill px-3 font-medium text-on-accent text-sm hover:brightness-110"
                    >
                      {r.review}
                    </a>
                  </Footer>
                </Match>
                <Match when={job().error}>
                  {(failure) => (
                    <>
                      <JobErrorNote error={failure()} />
                      <Footer>
                        <Button onClick={() => props.onClose()}>{l.close}</Button>
                        <Button
                          variant="primary"
                          disabled={pending()}
                          onClick={() => {
                            const current = progressing();
                            if (current) recover(failure().code, current.probe);
                          }}
                        >
                          {strings.job.error[failure().code].action}
                        </Button>
                      </Footer>
                    </>
                  )}
                </Match>
              </Switch>
            </div>
          )}
        </Match>
      </Switch>
    </div>
  );
}

function Footer(props: { children: JSX.Element }) {
  return <div class="mt-2 flex flex-wrap justify-end gap-2">{props.children}</div>;
}
