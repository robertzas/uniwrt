import { For, Match, Show, Switch } from "solid-js";
import type { Band } from "../../../api/gen/Band";
import type { Floor } from "../../../api/gen/Floor";
import { strings } from "../../../lib/strings";
import { Card, CardHeader } from "../../../ui/Card";
import { Segmented } from "../../../ui/Segmented";
import { formatArea } from "../units";
import type { CoverageState } from "./createCoverage";
import { STOPS } from "./layers";
import { metersPerPx } from "./model";

const t = strings.floors.coverage;
const OFF = "off";

/** The legend's gradient, as SVG stops (a constant inline style would be blocked by the CSP). */
function Legend() {
  const low = STOPS[0]?.value ?? -85;
  const high = STOPS[STOPS.length - 1]?.value ?? -55;
  return (
    <figure class="flex flex-col gap-1">
      <figcaption class="sr-only">{t.legend}</figcaption>
      <svg
        viewBox="0 0 100 8"
        preserveAspectRatio="none"
        class="h-2 w-full rounded-full"
        aria-hidden="true"
      >
        <defs>
          <linearGradient id="coverage-legend">
            <For each={STOPS}>
              {(stop) => (
                <stop
                  offset={`${((stop.value - low) / (high - low)) * 100}%`}
                  stop-color={`rgb(${stop.rgb.join(" ")})`}
                />
              )}
            </For>
          </linearGradient>
        </defs>
        <rect width="100" height="8" fill="url(#coverage-legend)" />
      </svg>
      <div class="flex justify-between text-fg-subtle text-xs">
        <span>{t.dead}</span>
        <span>{t.good}</span>
        <span>{t.strong}</span>
      </div>
    </figure>
  );
}

/**
 * The coverage beside the plan (features/wifi.md, Coverage & floor plans; D-118): off or a band,
 * the legend, the dead zones, where another node would help, and how the model was fitted.
 */
export function CoverageCard(props: {
  floor: Floor;
  band: Band | undefined;
  onBand: (band: Band | undefined) => void;
  state: CoverageState;
}) {
  const unit = () => props.floor.scale?.unit ?? "m";
  const options = () => {
    const bands = props.state.bands();
    const shown = bands.length > 0 ? bands : props.band ? [props.band] : (["5g"] as Band[]);
    return [
      { value: OFF, label: t.off },
      ...shown.map((band) => ({ value: band as string, label: t.band[band] })),
    ];
  };
  const model = () => props.state.model();
  const summary = () => props.state.summary();
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <div class="flex flex-col gap-3 px-4 pb-4 text-sm">
        <Segmented
          label={t.show}
          value={props.band ?? OFF}
          options={options()}
          onChange={(value) => props.onBand(value === OFF ? undefined : (value as Band))}
        />
        <Show when={props.band}>
          <Switch>
            <Match when={props.state.query.isError}>
              <p class="text-crit">{t.failed}</p>
            </Match>
            <Match when={!props.state.query.isSuccess}>
              <p role="status" class="text-fg-subtle">{`${t.loading}…`}</p>
            </Match>
            <Match when={!model()}>
              <p class="text-fg-muted">{t.noBands}</p>
            </Match>
            <Match when={!metersPerPx(props.floor)}>
              <p class="text-fg-muted">{t.needsScale}</p>
            </Match>
            <Match when={summary()?.placed === 0}>
              <p class="text-fg-muted">{t.noDevices}</p>
            </Match>
            <Match when={summary()}>
              {(s) => (
                <>
                  <Legend />
                  <p>
                    {s().dead > 0
                      ? t.deadZones(
                          formatArea(s().dead, unit()),
                          Math.round((s().dead / Math.max(s().area, 1e-9)) * 100),
                        )
                      : t.noDead}
                  </p>
                  <Show when={s().dead > 0}>
                    <p class="text-fg-muted">
                      {s().suggestion
                        ? t.suggestion(formatArea(s().suggestion?.gain ?? 0, unit()))
                        : t.noSuggestion}
                    </p>
                  </Show>
                  <ul class="flex flex-col gap-1 border-edge border-t pt-3 text-fg-subtle text-xs">
                    <li>
                      {model()?.exponent_fitted || model()?.floor_loss_fitted
                        ? t.fitted(model()?.samples ?? 0, model()?.error_db ?? 0)
                        : t.defaults}
                    </li>
                    <li>
                      {t.parameters(
                        model()?.exponent ?? 0,
                        model()?.floor_loss_db ?? 0,
                        model()?.floor_loss_fitted ?? false,
                      )}
                    </li>
                    <Show when={props.state.footprint()}>
                      {(f) => <li>{f().detected ? t.outline : t.wholePlan}</li>}
                    </Show>
                    <Show when={s().unplaced > 0}>
                      <li>{t.unplaced(s().unplaced)}</li>
                    </Show>
                  </ul>
                </>
              )}
            </Match>
          </Switch>
        </Show>
      </div>
    </Card>
  );
}
