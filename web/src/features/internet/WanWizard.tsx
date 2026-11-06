import { useNavigate } from "@solidjs/router";
import { ExternalLink } from "lucide";
import { createMemo, createSignal, For, Match, Show, Switch, untrack, useContext } from "solid-js";
import type { JobId } from "../../api/gen/JobId";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { Preset } from "../../api/gen/Preset";
import type { SiteView } from "../../api/gen/SiteView";
import type { WanConfig } from "../../api/gen/WanConfig";
import { ApiError, api, errorText } from "../../lib/api";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { check, comboError, type DraftErrors, errorsOfIssues } from "./check";
import {
  type Draft,
  draftOf,
  type Field,
  fallbackOf,
  fieldsOf,
  promptedFields,
  wanOf,
  withPreset,
} from "./draft";
import { IspPicker } from "./IspPicker";
import { ipv6Result } from "./summary";
import { TestPanel } from "./TestPanel";
import { AdvancedFields, FieldGrid } from "./WanFields";

const w = strings.internet.wizard;

type Step = "isp" | "settings" | "test" | "saved";
const STEPS: readonly Step[] = ["isp", "settings", "test", "saved"];

/**
 * The WAN wizard (wan.md, Wizard flow; D-075): ISP → only the fields the preset needs, with
 * every field under Advanced → Test (tried on the gateway, then rolled back) → saved as the
 * site's next revision, to review and apply. Mounted per opening.
 */
export function WanWizard(props: {
  site: SiteView;
  presets: readonly Preset[];
  gateway: NodeSummary | undefined;
  onClose: () => void;
}) {
  const live = useContext(LiveContext);
  const navigate = useNavigate();
  const [jobs, addJobs] = createJobsView(live);
  const current = untrack(() => props.site.config.wan);
  const storedPassword = untrack(() =>
    props.site.secrets.some((s) => s.path === "wan/password" && s.set),
  );
  const [step, setStep] = createSignal<Step>("isp");
  const [draft, setDraft] = createSignal<Draft>(draftOf(current));
  const [errors, setErrors] = createSignal<DraftErrors>({});
  const [failure, setFailure] = createSignal<string>();
  const [advanced, setAdvanced] = createSignal(untrack(() => current.preset === null));
  const [jobId, setJobId] = createSignal<JobId>();
  const [ipv6Off, setIpv6Off] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();

  const preset = createMemo(() => props.presets.find((p) => p.id === draft().preset));
  const job = () => {
    const id = jobId();
    return id === undefined ? undefined : jobs.byId[id];
  };
  const testable = () => props.gateway?.mode === "managed";
  const prompted = createMemo(() => {
    const p = preset();
    return p ? promptedFields(p) : [];
  });
  const rest = createMemo<Field[]>(() =>
    fieldsOf(draft()).filter((field) => !prompted().includes(field)),
  );

  const change = (next: Partial<Draft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setFailure(undefined);
  };
  const pick = (chosen: Preset) => {
    setDraft((now) => withPreset(now, chosen));
    setErrors({});
    setAdvanced(false);
    setStep("settings");
  };
  /** The form's WAN, or nothing after showing what is wrong. */
  const valid = (): WanConfig | undefined => {
    const found = check(draft(), storedPassword);
    const combo = comboError(draft());
    setErrors(found);
    if (Object.keys(found).length > 0 || combo) {
      if (Object.keys(found).some((field) => !prompted().includes(field as Field))) {
        setAdvanced(true);
      }
      if (combo) setAdvanced(true);
      setFailure(combo ?? strings.internet.errors.fix);
      return undefined;
    }
    return wanOf(draft(), current);
  };
  const refused = (err: unknown) => {
    if (err instanceof ApiError && err.issues.length > 0) {
      const { errors: found, other } = errorsOfIssues(err.issues);
      setErrors(found);
      setAdvanced(true);
      setFailure(other[0] ?? strings.internet.errors.fix);
      setStep("settings");
    } else {
      setFailure(errorText(err));
    }
  };

  const test = async () => {
    const wan = valid();
    if (!wan) return;
    setBusy(true);
    setIpv6Off(false);
    setJobId(undefined);
    setStep("test");
    try {
      const started = await api.wan.test({
        wan,
        password: draft().password === "" ? undefined : draft().password,
        fallback: fallbackOf(preset()),
      });
      addJobs([started]);
      setJobId(started.id);
    } catch (err) {
      refused(err);
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    const wan = valid();
    if (!wan) return;
    const finished = job();
    const fallback = fallbackOf(preset());
    if (finished?.state === "succeeded" && step() === "test") {
      if (ipv6Result(finished) === "sixrd" && fallback) wan.ipv6 = fallback;
      if (ipv6Result(finished) === "failed" && ipv6Off()) wan.ipv6 = { mode: "off" };
    }
    setBusy(true);
    try {
      const what = preset() ? `${preset()?.name} · ${preset()?.variant}` : strings.internet.custom;
      const secrets = draft().password === "" ? undefined : { "wan/password": draft().password };
      const view = await saveSite((config) => ({ ...config, wan }), w.message(what), secrets);
      setSaved(view.revision.id);
      setStep("saved");
    } catch (err) {
      refused(err);
    } finally {
      setBusy(false);
    }
  };

  const running = () => busy() || job()?.state === "running";

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !running() && props.onClose()}
      title={w.title}
      size="lg"
    >
      <div class="flex flex-col gap-5 px-5 pt-1 pb-5">
        <ol class="flex gap-1.5 text-xs" aria-label={w.title}>
          <For each={STEPS}>
            {(s, i) => (
              <li
                class={[
                  "flex items-center gap-1.5",
                  s === step() ? "font-medium text-fg" : "text-fg-subtle",
                ]}
                aria-current={s === step() ? "step" : undefined}
              >
                <Show when={i() > 0}>
                  <span aria-hidden="true">›</span>
                </Show>
                {w.steps[s]}
              </li>
            )}
          </For>
        </ol>

        <Switch>
          <Match when={step() === "isp"}>
            <IspPicker
              presets={props.presets}
              country={props.site.config.site.country}
              chosen={draft().preset}
              onPick={pick}
            />
          </Match>
          <Match when={step() === "settings"}>
            <div class="flex flex-col gap-4">
              <Show when={preset()} fallback={<p class="font-medium">{strings.internet.custom}</p>}>
                {(p) => (
                  <div>
                    <p class="font-medium">
                      {p().name} <span class="font-normal text-fg-muted">· {p().variant}</span>
                    </p>
                    <Show when={p().notes}>
                      <p class="mt-1 text-fg-muted text-sm">{p().notes}</p>
                    </Show>
                    <Show when={p().ipv6.mode === "auto"}>
                      <p class="mt-1 text-fg-muted text-sm">{w.auto}</p>
                    </Show>
                    <Show when={p().sources.length > 0}>
                      <p class="mt-1.5 flex flex-wrap gap-x-3 gap-y-1 text-xs">
                        <span class="text-fg-subtle">{w.sources}</span>
                        <For each={p().sources}>
                          {(url) => (
                            <a
                              href={url}
                              target="_blank"
                              rel="noreferrer noopener"
                              class="inline-flex items-center gap-1 text-accent hover:underline"
                            >
                              {new URL(url).hostname}
                              <Icon icon={ExternalLink} class="size-3" />
                            </a>
                          )}
                        </For>
                      </p>
                    </Show>
                  </div>
                )}
              </Show>
              <Show when={prompted().length > 0}>
                <FieldGrid
                  fields={prompted()}
                  draft={draft()}
                  errors={errors()}
                  storedPassword={storedPassword}
                  onChange={change}
                />
              </Show>
              <details
                open={advanced()}
                onToggle={(event) => setAdvanced(event.currentTarget.open)}
                class="rounded-control border border-edge"
              >
                <summary class="cursor-pointer select-none px-3 py-2 font-medium text-sm">
                  {w.advanced} <span class="font-normal text-fg-subtle">· {w.advancedHint}</span>
                </summary>
                <div class="border-edge border-t p-3">
                  <AdvancedFields
                    draft={draft()}
                    fields={rest()}
                    errors={errors()}
                    combo={comboError(draft())}
                    storedPassword={storedPassword}
                    onChange={change}
                  />
                </div>
              </details>
              <p class="text-fg-muted text-xs">{w.testHint}</p>
            </div>
          </Match>
          <Match when={step() === "test"}>
            <TestPanel job={job()} ipv6Off={ipv6Off()} onIpv6Off={setIpv6Off} />
          </Match>
          <Match when={step() === "saved"}>
            <div class="flex flex-col gap-1">
              <p class="font-medium">{w.saved(saved() ?? 0)}</p>
              <p class="text-fg-muted text-sm">{w.savedHint}</p>
            </div>
          </Match>
        </Switch>

        <RequestError text={failure()} />

        <div class="flex flex-wrap justify-end gap-2">
          <Switch>
            <Match when={step() === "isp"}>
              <Button variant="ghost" onClick={props.onClose}>
                {w.close}
              </Button>
              <Show when={draft().preset === null || preset()}>
                <Button variant="secondary" onClick={() => setStep("settings")}>
                  {preset() ? w.steps.settings : strings.internet.custom}
                </Button>
              </Show>
            </Match>
            <Match when={step() === "settings"}>
              <Button variant="ghost" onClick={() => setStep("isp")}>
                {w.back}
              </Button>
              <Button variant="secondary" disabled={busy()} onClick={save}>
                {w.saveUntested}
              </Button>
              <Button disabled={busy() || !testable()} onClick={test}>
                {w.test}
              </Button>
            </Match>
            <Match when={step() === "test"}>
              <Button variant="ghost" disabled={running()} onClick={() => setStep("settings")}>
                {w.back}
              </Button>
              <Show when={job()?.state === "failed"}>
                <Button variant="secondary" disabled={running()} onClick={test}>
                  {w.testAgain}
                </Button>
              </Show>
              <Button disabled={running() || job() === undefined} onClick={save}>
                {busy() ? w.saving : w.save}
              </Button>
            </Match>
            <Match when={step() === "saved"}>
              <Button variant="ghost" onClick={props.onClose}>
                {w.close}
              </Button>
              <Button onClick={() => navigate("/changes")}>{w.review}</Button>
            </Match>
          </Switch>
        </div>
      </div>
    </Dialog>
  );
}
