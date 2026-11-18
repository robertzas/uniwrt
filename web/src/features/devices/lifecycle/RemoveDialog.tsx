import { createSignal, Match, Show, Switch, useContext } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { JobId } from "../../../api/gen/JobId";
import type { Node } from "../../../api/gen/Node";
import { api, errorText } from "../../../lib/api";
import { createJobsView } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Checkbox } from "../../../ui/Checkbox";
import { Dialog } from "../../../ui/Dialog";
import { JobErrorNote, JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";
import { RootPasswordReveal } from "../config/RootPasswordCard";

const l = strings.device.lifecycle;

/** The actions that take a device out of the site's hands: for good, or until it is adopted back. */
export type Removal = "reset" | "decommission" | "forget";

/**
 * Confirms a reset, decommission or forget, says what each leaves behind, then follows its job.
 * A decommission or forget deletes the record at its end: the device page then gives way to
 * its outcome (`Removed`), closing this dialog with it.
 */
export function RemoveDialog(props: {
  node: Node;
  action: Removal | undefined;
  onClose: () => void;
}) {
  return (
    <Dialog
      open={props.action !== undefined}
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={props.action ? l[props.action].title(props.node.name) : ""}
    >
      <Show when={props.action}>
        {(action) => <Body node={props.node} action={action()} onClose={props.onClose} />}
      </Show>
    </Dialog>
  );
}

/** The dialog's body; mounted on open, so every attempt starts afresh. */
function Body(props: { node: Node; action: Removal; onClose: () => void }) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [firstboot, setFirstboot] = createSignal(false);
  const [started, setStarted] = createSignal<JobId>();
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const job = () => jobs.byId[started() ?? -1];

  const request = (): Promise<Job> => {
    const id = props.node.id;
    switch (props.action) {
      case "reset":
        return api.nodes.reset(id);
      case "decommission":
        return api.nodes.decommission(id, { firstboot: firstboot() });
      case "forget":
        return api.nodes.forget(id);
    }
  };
  const start = async () => {
    if (pending()) return;
    setPending(true);
    setError(undefined);
    try {
      const started = await request();
      addJobs([started]);
      setStarted(started.id);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <div class="flex flex-col gap-4 px-5 pt-1 pb-5 text-sm">
      <Show
        when={job()}
        fallback={
          <form
            class="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              void start();
            }}
          >
            <Switch>
              <Match when={props.action === "reset"}>
                <p class="text-fg-muted">{l.reset.body(props.node.mac, props.node.board_name)}</p>
                <p class="text-fg-muted">{l.reset.offline}</p>
              </Match>
              <Match when={props.action === "decommission"}>
                <p class="text-fg-muted">{l.decommission.body}</p>
                <p class="text-fg-muted">{l.decommission.reach}</p>
                <Checkbox
                  label={l.decommission.firstboot}
                  hint={l.decommission.firstbootHint}
                  checked={firstboot()}
                  onChange={setFirstboot}
                />
                <Show when={!firstboot()}>
                  <div class="rounded-control border border-warn/40 bg-warn/5 px-3.5 py-3">
                    <RootPasswordReveal node={props.node} hint={l.decommission.password} />
                  </div>
                </Show>
              </Match>
              <Match when={props.action === "forget"}>
                <p class="text-fg-muted">{l.forget.body}</p>
              </Match>
            </Switch>
            <RequestError text={error()} />
            <div class="flex flex-wrap justify-end gap-2">
              <Button variant="ghost" onClick={() => props.onClose()}>
                {l.cancel}
              </Button>
              <Button type="submit" variant="primary" disabled={pending()}>
                {l[props.action].submit}
              </Button>
            </div>
          </form>
        }
      >
        {(job) => (
          <>
            <JobSteps job={job()} />
            <Show when={job().state === "succeeded" && props.action === "reset"}>
              <JobSuccessNote text={l.reset.succeeded} />
            </Show>
            <Show when={job().error}>{(failure) => <JobErrorNote error={failure()} />}</Show>
            <div class="flex justify-end">
              <Button onClick={() => props.onClose()}>{l.close}</Button>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}
