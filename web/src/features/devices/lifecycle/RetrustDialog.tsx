import type { JSX } from "@solidjs/web";
import { createSignal, Match, onSettled, Show, Switch, useContext } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { JobErrorCode } from "../../../api/gen/JobErrorCode";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import type { ProbeResult } from "../../../api/gen/ProbeResult";
import type { RetrustBody } from "../../../api/gen/RetrustBody";
import { api, errorText } from "../../../lib/api";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { JobErrorNote, JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";
import { TextField } from "../../../ui/TextField";
import { HostKey } from "../adopt/HostKey";

const a = strings.adopt;
const l = strings.device.lifecycle;
const r = l.retrust;

/** What the node's status says of its keys: the one pinned, and the one it presented. */
export type KeyChange = { pinned: string; presented: string };

/**
 * Trusts a node's changed SSH host key (D-154): the key the device at the node's address
 * presents now, read again and compared beside the pinned one; then a job that checks the device
 * is the node's own before pinning the key. A device that no longer trusts uniwrt's key was
 * reset: its root password adopts it back into the record, in observe mode.
 */
export function RetrustDialog(props: {
  node: Node;
  change: KeyChange;
  open: boolean;
  onClose: () => void;
}) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={r.title(props.node.name)}
    >
      <Wizard node={props.node} change={props.change} onClose={props.onClose} />
    </Dialog>
  );
}

type Stage =
  | { kind: "reading" }
  | { kind: "confirm"; probe: ProbeResult }
  | { kind: "password"; probe: ProbeResult }
  | { kind: "progress"; probe: ProbeResult; job: JobId; withPassword: boolean };

/** Whether the job adopted the device back (it installed uniwrt's key) rather than kept it. */
export function readopted(job: Job): boolean {
  return job.steps.some((step) => step.name === "install_key" && step.state === "done");
}

function Wizard(props: { node: Node; change: KeyChange; onClose: () => void }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [stage, setStage] = createSignal<Stage>({ kind: "reading" });
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
      setStage({ kind: "reading" });
      const probe = await api.adopt.probe({ host: props.node.host, port: props.node.port });
      setStage({ kind: "confirm", probe });
    });
  onSettled(() => void readKey());

  /** Starts the job; with `withPassword` the root password asked for, `anyway` overriding the release check (D-145). */
  const trust = (probe: ProbeResult, withPassword: boolean, anyway = false) =>
    run(async () => {
      const body: RetrustBody = { host_key: probe.host_key };
      if (anyway) body.allow_unsupported_release = true;
      if (withPassword) {
        body.password = password();
        if (password() && remember()) body.remember_password = true;
      }
      const job = await api.nodes.retrust(props.node.id, body);
      addJobs([job]);
      setStage({ kind: "progress", probe, job: job.id, withPassword });
    });

  /** What a failure's action does: ask for the password, read the key again, or retry. */
  const recover = (code: JobErrorCode, probe: ProbeResult, withPassword: boolean) => {
    switch (code) {
      case "auth_rejected":
        setPassword("");
        setStage({ kind: "password", probe });
        break;
      case "host_key_mismatch":
        void readKey();
        break;
      case "unsupported_release":
        void trust(probe, withPassword, true);
        break;
      case "already_adopted":
      case "identity_mismatch":
        props.onClose();
        break;
      default:
        void trust(probe, withPassword);
    }
  };

  const confirming = () => {
    const current = stage();
    return current.kind === "confirm" ? current.probe : undefined;
  };
  const asking = () => {
    const current = stage();
    return current.kind === "password" ? current.probe : undefined;
  };
  const progressing = () => {
    const current = stage();
    return current.kind === "progress" ? current : undefined;
  };

  return (
    <div class="px-5 pt-1 pb-5">
      <Switch>
        <Match when={stage().kind === "reading"}>
          <div class="flex flex-col gap-4">
            <p class="text-fg-muted text-sm" role="status">
              {r.reading}
            </p>
            <RequestError text={error()} />
            <Footer>
              <Button variant="ghost" onClick={() => props.onClose()}>
                {l.cancel}
              </Button>
              <Show when={error()}>
                <Button variant="primary" disabled={pending()} onClick={() => void readKey()}>
                  {strings.job.error.host_key_mismatch.action}
                </Button>
              </Show>
            </Footer>
          </div>
        </Match>
        <Match when={confirming()}>
          {(probe) => (
            <form
              class="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                void trust(probe(), false);
              }}
            >
              <p class="text-fg-muted text-sm">{r.description}</p>
              <p class="text-fg-muted text-sm">{r.untouched}</p>
              <RequestError text={error()} />
              <div class="rounded-control border border-edge px-3.5 py-3">
                <p class="text-fg-muted text-xs">{r.pinned}</p>
                <p
                  class="mt-1.5 break-all font-mono text-fg-subtle text-sm line-through"
                  data-testid="pinned"
                >
                  {props.change.pinned}
                </p>
              </div>
              <HostKey probe={probe()} />
              <Switch>
                <Match when={probe().fingerprint === props.change.pinned}>
                  <p class="text-fg-muted text-sm" role="note">
                    {r.same}
                  </p>
                </Match>
                <Match when={probe().fingerprint !== props.change.presented}>
                  <p class="text-sm text-warn" role="alert">
                    {r.changedAgain}
                  </p>
                </Match>
              </Switch>
              <Footer>
                <Button variant="ghost" onClick={() => props.onClose()}>
                  {l.cancel}
                </Button>
                <Button type="submit" variant="primary" disabled={pending()}>
                  {pending() ? a.confirm.starting : r.submit}
                </Button>
              </Footer>
            </form>
          )}
        </Match>
        <Match when={asking()}>
          {(probe) => (
            <form
              class="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                void trust(probe(), true);
              }}
            >
              <div>
                <h3 class="font-medium text-sm">{r.password.title}</h3>
                <p class="mt-1 text-fg-muted text-sm">{r.password.description}</p>
              </div>
              <RequestError text={error()} />
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
                  {pending() ? a.confirm.starting : r.password.submit}
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
                  <JobSuccessNote text={readopted(job()) ? r.readopted : r.trusted} />
                  <Footer>
                    <Button onClick={() => props.onClose()}>{l.close}</Button>
                  </Footer>
                </Match>
                <Match when={job().error}>
                  {(failure) => (
                    <>
                      <JobErrorNote error={failure()} />
                      <Footer>
                        <Button onClick={() => props.onClose()}>{l.close}</Button>
                        <Show
                          when={
                            failure().code !== "identity_mismatch" &&
                            failure().code !== "already_adopted"
                          }
                        >
                          <Button
                            variant="primary"
                            disabled={pending()}
                            onClick={() => {
                              const current = progressing();
                              if (current)
                                recover(failure().code, current.probe, current.withPassword);
                            }}
                          >
                            {strings.job.error[failure().code].action}
                          </Button>
                        </Show>
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

/** The status line's action: checking the new key, in its dialog. */
export function RetrustAction(props: { node: Node; change: KeyChange }) {
  const [open, setOpen] = createSignal(false);
  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        {r.action}
      </Button>
      <Show when={open()}>
        <RetrustDialog
          node={props.node}
          change={props.change}
          open={open()}
          onClose={() => setOpen(false)}
        />
      </Show>
    </>
  );
}

function Footer(props: { children: JSX.Element }) {
  return <DialogFooter class="mt-2">{props.children}</DialogFooter>;
}
