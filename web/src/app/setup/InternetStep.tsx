import { Match, Show, Switch } from "solid-js";
import type { Preset } from "../../api/gen/Preset";
import { IspPicker } from "../../features/internet/IspPicker";
import { TestPanel } from "../../features/internet/TestPanel";
import { WanSettings } from "../../features/internet/WanSettings";
import type { WanForm } from "../../features/internet/wanForm";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { RequestError } from "../../ui/JobOutcome";

const t = strings.setup.internet;

/**
 * The wizard's internet step (wan.md, Wizard flow): the ISP, the fields its preset asks for,
 * and an optional test on the router (D-082). Untested settings are fine: the apply connects
 * last and undoes only them when they fail (D-081).
 */
export function InternetStep(props: {
  form: WanForm;
  presets: readonly Preset[];
  /** The site's country, which the ISP list starts with. */
  country: string | null;
  storedPassword: boolean;
  /** The ISP list is shown, not the settings. */
  picking: boolean;
  onPicking: (picking: boolean) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const form = () => props.form;
  const picking = () => props.picking;
  const setPicking = (picking: boolean) => props.onPicking(picking);
  const tested = () => form().job() !== undefined || form().busy();

  const next = () => {
    if (form().valid()) props.onNext();
  };

  return (
    <div class="flex flex-col gap-5">
      <h2 class="font-semibold text-lg tracking-tight">{t.heading}</h2>
      <Switch>
        <Match when={picking()}>
          <IspPicker
            presets={props.presets}
            country={props.country}
            chosen={form().draft().preset}
            onPick={(preset) => {
              form().pick(preset);
              setPicking(false);
            }}
          />
        </Match>
        <Match when={true}>
          <WanSettings form={form()} storedPassword={props.storedPassword} />
          <Show when={tested()} fallback={<p class="text-fg-muted text-xs">{t.testHint}</p>}>
            <TestPanel
              job={form().job()}
              ipv6Off={form().ipv6Off()}
              onIpv6Off={form().setIpv6Off}
            />
          </Show>
        </Match>
      </Switch>

      <RequestError text={form().failure()} />

      <div class="flex flex-wrap justify-end gap-2">
        <Button variant="ghost" disabled={form().running()} onClick={props.onBack}>
          {strings.setup.back}
        </Button>
        <Switch>
          <Match when={picking()}>
            <Button
              variant="secondary"
              onClick={() => {
                form().setAdvanced(true);
                setPicking(false);
              }}
            >
              {form().preset() ? strings.internet.wizard.steps.settings : t.custom}
            </Button>
          </Match>
          <Match when={true}>
            <Button variant="ghost" disabled={form().running()} onClick={() => setPicking(true)}>
              {t.change}
            </Button>
            <Button variant="secondary" disabled={form().running()} onClick={() => form().test()}>
              {form().job() ? t.testAgain : t.test}
            </Button>
            <Button variant="primary" disabled={form().running()} onClick={next}>
              {strings.setup.next}
            </Button>
          </Match>
        </Switch>
      </div>
    </div>
  );
}
