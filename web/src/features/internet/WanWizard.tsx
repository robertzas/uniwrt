import { useNavigate } from "@solidjs/router";
import { createSignal, For, Match, Show, Switch, untrack, useContext } from "solid-js";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { Preset } from "../../api/gen/Preset";
import type { SiteView } from "../../api/gen/SiteView";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { IspPicker } from "./IspPicker";
import { TestPanel } from "./TestPanel";
import { WanSettings } from "./WanSettings";
import { createWanForm } from "./wanForm";

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
  const storedPassword = untrack(() =>
    props.site.secrets.some((s) => s.path === "wan/password" && s.set),
  );
  const form = createWanForm({
    current: untrack(() => props.site.config.wan),
    storedPassword,
    presets: () => props.presets,
    jobs,
    addJobs,
  });
  const { draft, preset, job, failure, busy, running } = form;
  const [step, setStep] = createSignal<Step>("isp");
  const [saved, setSaved] = createSignal<number>();

  const testable = () => props.gateway?.mode === "managed";
  const pick = (chosen: Preset) => {
    form.pick(chosen);
    setStep("settings");
  };
  const test = () => form.test(() => setStep("test"));
  const save = async () => {
    const wan = form.outcome(step() === "test");
    if (!wan) return;
    form.setBusy(true);
    try {
      const view = await saveSite(
        (config) => ({ ...config, wan }),
        w.message(form.label()),
        form.secrets(),
      );
      setSaved(view.revision.id);
      setStep("saved");
    } catch (err) {
      if (form.refused(err)) setStep("settings");
    } finally {
      form.setBusy(false);
    }
  };

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
              <WanSettings form={form} storedPassword={storedPassword} />
              <p class="text-fg-muted text-xs">{w.testHint}</p>
            </div>
          </Match>
          <Match when={step() === "test"}>
            <TestPanel job={job()} ipv6Off={form.ipv6Off()} onIpv6Off={form.setIpv6Off} />
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
