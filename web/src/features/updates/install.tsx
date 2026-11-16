import { Download } from "lucide";
import { createEffect, createMemo, createSignal, Show, useContext } from "solid-js";
import type { Job } from "../../api/gen/Job";
import type { JobId } from "../../api/gen/JobId";
import type { UpgradeInstallBody } from "../../api/gen/UpgradeInstallBody";
import { api, errorText } from "../../lib/api";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { JobErrorNote, JobSuccessNote, RequestError } from "../../ui/JobOutcome";
import { JobSteps } from "../../ui/JobSteps";
import { showToast } from "../../ui/Toast";

const d = strings.firmware.dialog;

export type Install = ReturnType<typeof createInstall>;

/**
 * Installing a prepared upgrade on a node (D-124): asked, confirmed, then the job followed live
 * on the `jobs` topic, as are upgrades of the node started elsewhere.
 */
export function createInstall(props: {
  node: () => number;
  name: () => string;
  /** What installs the chosen image; `undefined` until it can be (ASU's build unfinished). */
  body: () => UpgradeInstallBody | undefined;
  managed: () => boolean;
}) {
  const live = useContext(LiveContext);
  const [jobs, addJobs] = createJobsView(live);
  const [asking, setAsking] = createSignal(false);
  const [sending, setSending] = createSignal(false);
  const [started, setStarted] = createSignal<JobId>();
  const [error, setError] = createSignal<string>();

  /** The upgrade started here, else one running on this node. */
  const job = createMemo(() => {
    const own = started();
    if (own !== undefined && jobs.byId[own]) return jobs.byId[own];
    return Object.values(jobs.byId).find(
      (job) => job.kind === "upgrade" && job.node === props.node() && job.state === "running",
    );
  });
  const running = () => sending() || job()?.state === "running";

  createEffect(
    () => {
      const own = started();
      return own === undefined ? undefined : jobs.byId[own]?.state;
    },
    (state) => {
      const own = jobs.byId[started() ?? -1];
      if (!own || state === undefined || state === "running") return;
      if (state === "succeeded") {
        showToast({ title: d.done(props.name()), tone: "ok" });
      } else {
        showToast({ title: d.failed(props.name()), description: own.error?.message, tone: "crit" });
      }
    },
  );

  const send = async () => {
    const body = props.body();
    if (!body) return;
    setSending(true);
    setError(undefined);
    try {
      const job = await api.firmware.install(props.node(), body);
      addJobs([job]);
      setStarted(job.id);
      setAsking(false);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSending(false);
    }
  };

  return {
    job,
    running,
    asking,
    setAsking,
    sending,
    error,
    send,
    /** An upgrade started, or runs: the choices are settled. */
    busy: () => running() || job() !== undefined,
    /** Install can be asked for now. */
    ready: () => props.body() !== undefined && props.managed() && job() === undefined,
    managed: props.managed,
  };
}

/** What the install says in the dialog: why it can't, the confirmation, the job as it goes. */
export function InstallStatus(props: { install: Install; name: string; official: boolean }) {
  const i = () => props.install;
  return (
    <>
      <Show when={!i().managed()}>
        <p class="text-fg-subtle text-xs">{d.observe}</p>
      </Show>
      <Show when={i().asking() && !i().job()}>
        <div
          role="alert"
          class="flex flex-col gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm"
        >
          <p>{d.confirm(props.name)}</p>
          <Show when={props.official}>
            <p>{d.confirmPackages}</p>
          </Show>
          <p class="text-fg-muted">{d.confirmSoak}</p>
        </div>
      </Show>
      <RequestError text={i().error()} />
      <Show when={i().job()}>
        {(job) => (
          <div class="flex flex-col gap-2">
            <div class="rounded-control border border-edge p-2" role="status">
              <JobSteps job={job() as Job} />
            </div>
            <Show when={job().state === "running"}>
              <p class="text-fg-muted text-xs">{d.running}</p>
            </Show>
            <Show when={job().state === "succeeded"}>
              <JobSuccessNote text={d.done(props.name)} />
            </Show>
            <Show when={job().error}>{(error) => <JobErrorNote error={error()} />}</Show>
          </div>
        )}
      </Show>
    </>
  );
}

/** Install, then Install now once asked (with Cancel). */
export function InstallButton(props: { install: Install }) {
  const i = () => props.install;
  return (
    <Show when={!i().job()}>
      <Show
        when={i().asking()}
        fallback={
          <Button
            size="sm"
            variant="primary"
            icon={Download}
            disabled={!i().ready()}
            onClick={() => i().setAsking(true)}
          >
            {d.install}
          </Button>
        }
      >
        <Button size="sm" onClick={() => i().setAsking(false)}>
          {d.cancel}
        </Button>
        <Button
          size="sm"
          variant="primary"
          icon={Download}
          disabled={i().sending()}
          onClick={() => void i().send()}
        >
          {i().sending() ? d.installing : d.installNow}
        </Button>
      </Show>
    </Show>
  );
}
