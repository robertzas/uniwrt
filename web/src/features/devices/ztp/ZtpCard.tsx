import { BadgeCheck, Cable, Fingerprint, LoaderCircle, Lock, PackagePlus, X } from "lucide";
import { createSignal, For, Match, onSettled, Show, Switch } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { PendingDevice } from "../../../api/gen/PendingDevice";
import type { ZtpApproval } from "../../../api/gen/ZtpApproval";
import type { ZtpInbox } from "../../../api/gen/ZtpInbox";
import { api, errorText } from "../../../lib/api";
import type { JobsState } from "../../../lib/jobs";
import { relative } from "../../../lib/rf";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { JobErrorNote, RequestError } from "../../../ui/JobOutcome";
import { currentStep, jobProgress } from "../../../ui/JobSteps";
import { SetupDialog } from "./SetupDialog";
import { ApproveDialog, ZtpAdoptDialog } from "./ZtpDialogs";

const t = strings.ztp;
/** How often the inbox is read again while the page is open. */
const POLL_MS = 5_000;

/** `SHA256:abcdefgh…wxyz`, for a list; the adopt dialog shows the whole fingerprint. */
export function shortFingerprint(fingerprint: string): string {
  const [algorithm, hash = ""] = fingerprint.split(":", 2);
  return hash.length > 14 ? `${algorithm}:${hash.slice(0, 8)}…${hash.slice(-4)}` : fingerprint;
}

/**
 * `GET v1/ztp/pending` now and again while the calling component lives, skipped while the tab is
 * hidden; `refresh` reads it at once (after an action). A plain signal, as discovery's: a
 * pending query would hold navigations away from the page.
 */
function pollInbox() {
  const [inbox, setInbox] = createSignal<ZtpInbox>();
  const [error, setError] = createSignal<string>();
  const [now, setNow] = createSignal(Date.now());
  let refresh = async () => {};
  onSettled(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = async () => {
      try {
        setInbox(await api.ztp.inbox(controller.signal));
        setError(undefined);
      } catch (err) {
        if (!controller.signal.aborted) setError(errorText(err));
      }
      setNow(Date.now());
    };
    const poll = async () => {
      if (!document.hidden) await read();
      if (!controller.signal.aborted) timer = setTimeout(poll, POLL_MS);
    };
    refresh = read;
    void poll();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  });
  return { inbox, error, now, refresh: () => refresh() };
}

/**
 * The pending adoption inbox (adoption.md, Paths B and C): devices that called home or that
 * answer on the gateway's adoption port, each adopted after its host key is checked or
 * dismissed, and the MACs pre-approved to be adopted at their first hello. Admins only.
 */
export function ZtpCard(props: { jobs: JobsState; addJob: (job: Job) => void }) {
  const { inbox, error, now, refresh } = pollInbox();
  const [adopting, setAdopting] = createSignal<PendingDevice>();
  const [approving, setApproving] = createSignal(false);
  const [settingUp, setSettingUp] = createSignal(false);
  const [actionError, setActionError] = createSignal<string>();
  const pending = () => inbox()?.pending ?? [];
  const approvals = () => inbox()?.approvals ?? [];
  const act = async (action: () => Promise<unknown>) => {
    setActionError(undefined);
    try {
      await action();
    } catch (err) {
      setActionError(errorText(err));
    }
    await refresh();
  };

  return (
    <Card class="mx-auto mt-4 max-w-7xl overflow-hidden" aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <>
            <Button size="sm" variant="ghost" onClick={() => setApproving(true)}>
              {t.approvals.add}
            </Button>
            <Button size="sm" icon={PackagePlus} onClick={() => setSettingUp(true)}>
              {t.add}
            </Button>
          </>
        }
      />
      <div class="px-4">
        <RequestError text={actionError() ?? (inbox() ? undefined : error())} />
      </div>
      <Show
        when={pending().length > 0}
        fallback={
          <Show when={inbox()}>
            <p class="border-edge border-t px-4 py-3 text-fg-subtle text-sm">{t.empty}</p>
          </Show>
        }
      >
        <ul class="divide-y divide-edge border-edge border-t" aria-label={t.title}>
          <For each={pending()} keyed={(device) => device.mac}>
            {(device) => (
              <Row
                device={device()}
                jobs={props.jobs}
                now={now()}
                onAdopt={() => setAdopting(device())}
                onDismiss={() => act(() => api.ztp.dismiss(device().mac))}
              />
            )}
          </For>
        </ul>
      </Show>
      <Show when={approvals().length > 0}>
        <section class="border-edge border-t px-4 py-3" aria-label={t.approvals.title}>
          <h3 class="font-medium text-sm">{t.approvals.title}</h3>
          <p class="text-fg-subtle text-xs">{t.approvals.description}</p>
          <ul class="mt-2 flex flex-col gap-1.5">
            <For each={approvals()} keyed={(approval) => approval.mac}>
              {(approval) => (
                <Approval
                  approval={approval()}
                  now={now()}
                  onWithdraw={() => act(() => api.ztp.withdraw(approval().mac))}
                />
              )}
            </For>
          </ul>
        </section>
      </Show>
      <ZtpAdoptDialog
        device={adopting()}
        onClose={() => setAdopting(undefined)}
        onStarted={(job) => {
          props.addJob(job);
          setAdopting(undefined);
          void refresh();
        }}
      />
      <SetupDialog
        open={settingUp()}
        onOpenChange={setSettingUp}
        onCalledHome={() => void refresh()}
      />
      <ApproveDialog
        open={approving()}
        onClose={() => setApproving(false)}
        onSaved={() => {
          setApproving(false);
          void refresh();
        }}
      />
    </Card>
  );
}

function Row(props: {
  device: PendingDevice;
  jobs: JobsState;
  now: number;
  onAdopt: () => void;
  onDismiss: () => void;
}) {
  const job = () => (props.device.job === null ? undefined : props.jobs.byId[props.device.job]);
  const name = () => props.device.hostname || t.unnamed;
  const port = () => (props.device.source.kind === "port" ? props.device.source.port : undefined);
  const dismissHint = () => (port() === undefined ? t.dismissHint : t.port.dismissHint);
  const adopt = () => (
    <Button
      size="sm"
      onClick={props.onAdopt}
      disabled={port() !== undefined}
      title={port() === undefined ? undefined : t.port.soon}
    >
      {job()?.state === "failed" ? t.retry : t.adopt}
    </Button>
  );
  const dismiss = () => (
    <Button size="sm" variant="ghost" icon={X} title={dismissHint()} onClick={props.onDismiss}>
      {t.dismiss}
    </Button>
  );
  return (
    <li class="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2.5">
      <div class="flex min-w-56 flex-1 items-center gap-2.5">
        <Icon
          icon={port() === undefined ? PackagePlus : Cable}
          class="size-4 shrink-0 text-fg-subtle"
        />
        <div class="min-w-0">
          <p class="truncate font-medium text-sm">
            {name()}
            <span class="ml-2 font-normal text-fg-muted">{props.device.model}</span>
            <Show when={props.device.locked}>
              <span class="ml-2 inline-flex align-middle" title={t.port.lockedHint}>
                <Badge icon={Lock} tone="warn">
                  {t.port.locked}
                </Badge>
              </span>
            </Show>
          </p>
          <p class="truncate text-fg-subtle text-xs">
            <span class="font-mono">{props.device.mac}</span>
            {" · "}
            <Show when={port()} fallback={t.seen(relative(props.device.last_seen, props.now))}>
              {(name) =>
                `${t.port.on(name())} · ${t.port.seen(relative(props.device.last_seen, props.now))}`
              }
            </Show>
            <Show when={props.device.role}>
              {(role) => ` · ${t.builtFor(strings.devices.role[role()])}`}
            </Show>
          </p>
        </div>
      </div>
      <div class="w-32 font-mono text-xs">{props.device.address}</div>
      <div class="flex w-56 min-w-0 items-center gap-1.5 text-xs">
        <Icon icon={Fingerprint} class="size-3.5 shrink-0 text-fg-subtle" />
        <span class="truncate font-mono" title={props.device.fingerprint}>
          {shortFingerprint(props.device.fingerprint)}
        </span>
      </div>
      <div class="ml-auto flex items-center justify-end gap-2">
        <Switch
          fallback={
            <>
              {dismiss()}
              {adopt()}
            </>
          }
        >
          <Match when={job()?.state === "running" && job()}>
            {(running) => (
              <Badge icon={LoaderCircle} tone="info">
                {t.adopting} · {Math.round(jobProgress(running()) * 100)}%
                <Show when={currentStep(running())}>
                  {(step) => <span class="sr-only">{strings.job.step[step().name]}</span>}
                </Show>
              </Badge>
            )}
          </Match>
          <Match when={job()?.state === "failed"}>
            {dismiss()}
            {adopt()}
          </Match>
        </Switch>
      </div>
      <Show when={job()?.state === "failed" && job()?.error}>
        {(failure) => (
          <div class="basis-full">
            <JobErrorNote error={failure()} compact />
          </div>
        )}
      </Show>
    </li>
  );
}

function Approval(props: { approval: ZtpApproval; now: number; onWithdraw: () => void }) {
  const a = () => props.approval;
  return (
    <li class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
      <Icon icon={BadgeCheck} class="size-4 shrink-0 text-ok" />
      <span class="font-mono text-xs">{a().mac}</span>
      <Show when={a().name}>{(name) => <span class="font-medium">{name()}</span>}</Show>
      <Show when={a().role}>
        {(role) => <Badge tone="neutral">{strings.devices.role[role()]}</Badge>}
      </Show>
      <span class="text-fg-subtle text-xs">
        {t.approvals.expires(relative(a().expires_at, props.now))}
        <Show when={a().created_by}>{(user) => ` · ${t.approvals.by(user())}`}</Show>
      </span>
      <Button size="sm" variant="ghost" class="ml-auto" onClick={props.onWithdraw}>
        {t.approvals.withdraw}
      </Button>
    </li>
  );
}
