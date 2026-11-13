import { createEffect, createSignal, Show } from "solid-js";
import type { Floor } from "../../api/gen/Floor";
import type { LengthUnit } from "../../api/gen/LengthUnit";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { FormField, INPUT, SELECT } from "../../ui/FormField";
import { RequestError } from "../../ui/JobOutcome";
import { TextField } from "../../ui/TextField";
import { showToast } from "../../ui/Toast";
import { nextLevel } from "./floors";
import { fromMeters, toMeters } from "./units";
import { normalizePlan, PlanError } from "./upload";

const t = strings.floors.dialog;

type Errors = Partial<Record<"name" | "level" | "height" | "plan", string>>;

/**
 * Adds a floor or edits one (`floor`): its name, level, floor-to-floor height (in m or ft) and
 * plan, normalized in the browser before upload; editing also deletes it, after a confirmation.
 */
export function FloorDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  floor: Floor | undefined;
  floors: readonly Floor[];
  onSaved: (floor: Floor) => void;
  onDeleted: (id: number) => void;
}) {
  const [name, setName] = createSignal("");
  const [level, setLevel] = createSignal("0");
  const [height, setHeight] = createSignal("3");
  const [unit, setUnit] = createSignal<LengthUnit>("m");
  const [plan, setPlan] = createSignal<File>();
  const [errors, setErrors] = createSignal<Errors>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);

  // Each opening starts from the floor edited, or a new floor above the highest.
  createEffect(
    () => (props.open ? (props.floor ?? null) : undefined),
    (floor) => {
      if (floor === undefined) return;
      const shown = floor?.scale?.unit ?? "m";
      setName(floor?.name ?? "");
      setLevel(String(floor?.level ?? nextLevel(props.floors)));
      setUnit(shown);
      setHeight(String(Math.round(fromMeters(floor?.height_m ?? 3, shown) * 10) / 10));
      setPlan(undefined);
      setErrors({});
      setFailure(undefined);
      setConfirming(false);
    },
  );

  const check = () => {
    const found: Errors = {};
    const levelValue = Number(level());
    const meters = toMeters(Number(height()), unit());
    if (!name().trim() || name().trim().length > 64) found.name = t.invalidName;
    if (!Number.isInteger(levelValue) || levelValue < -9 || levelValue > 99) {
      found.level = t.invalidLevel;
    }
    if (!(meters >= 2 && meters <= 10)) found.height = t.invalidHeight;
    setErrors(found);
    return Object.keys(found).length === 0 ? { level: levelValue, meters } : undefined;
  };

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const valid = check();
    if (!valid) return;
    setBusy(true);
    setFailure(undefined);
    try {
      // The plan is read before anything is saved, so a file the browser can't open saves nothing.
      const file = plan();
      const image = file ? await normalizePlan(file) : undefined;
      const fields = {
        name: name().trim(),
        level: valid.level,
        height_m: Math.round(valid.meters * 100) / 100,
        scale: props.floor?.scale ?? null,
      };
      let floor = props.floor
        ? await api.floors.update(props.floor.id, fields)
        : await api.floors.create(fields);
      props.onSaved(floor);
      if (image) {
        floor = await api.floors.upload(floor.id, image);
        props.onSaved(floor);
      }
      showToast({ title: props.floor ? t.saved : t.added, tone: "ok" });
      props.onOpenChange(false);
    } catch (err) {
      if (err instanceof PlanError) {
        setErrors({ plan: t.planErrors[err.message as keyof typeof t.planErrors] ?? t.failed });
      } else {
        setFailure(errorText(err) ?? t.failed);
      }
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    const floor = props.floor;
    if (!floor) return;
    setBusy(true);
    try {
      await api.floors.remove(floor.id);
      props.onDeleted(floor.id);
      showToast({ title: t.deleted, tone: "ok" });
      props.onOpenChange(false);
    } catch (err) {
      setFailure(errorText(err) ?? t.failed);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={props.floor ? t.editTitle : t.addTitle}
      description={props.floor ? t.editDescription : t.addDescription}
    >
      <form class="flex flex-col gap-4 px-5 pt-2 pb-5" onSubmit={submit} novalidate>
        <TextField
          label={t.name}
          value={name()}
          onInput={setName}
          error={errors().name}
          autofocus
        />
        <div class="grid grid-cols-2 gap-3">
          <FormField label={t.level} for="floor-level" error={errors().level}>
            <input
              id="floor-level"
              class={INPUT}
              type="number"
              step="1"
              min="-9"
              max="99"
              value={level()}
              onInput={(event) => setLevel(event.currentTarget.value)}
            />
          </FormField>
          <FormField label={t.height} for="floor-height" error={errors().height}>
            <div class="flex gap-2">
              <input
                id="floor-height"
                class={INPUT}
                type="number"
                step="any"
                inputmode="decimal"
                value={height()}
                onInput={(event) => setHeight(event.currentTarget.value)}
              />
              <select
                class={[SELECT, "w-20"]}
                aria-label={strings.floors.scale.unit}
                value={unit()}
                onChange={(event) => {
                  const next = event.currentTarget.value as LengthUnit;
                  const meters = toMeters(Number(height()), unit());
                  if (Number.isFinite(meters)) {
                    setHeight(String(Math.round(fromMeters(meters, next) * 10) / 10));
                  }
                  setUnit(next);
                }}
              >
                <option value="m">m</option>
                <option value="ft">ft</option>
              </select>
            </div>
          </FormField>
        </div>
        <p class="-mt-2 text-fg-subtle text-xs">{`${t.levelHint} ${t.heightHint}`}</p>
        <FormField
          label={props.floor?.image ? t.replacePlan : t.plan}
          for="floor-plan"
          hint={t.planHint}
          error={errors().plan}
        >
          <input
            id="floor-plan"
            type="file"
            accept="image/*"
            class="text-sm file:mr-3 file:h-9 file:cursor-pointer file:rounded-control file:border file:border-edge file:bg-surface file:px-3 file:font-medium file:text-fg file:text-sm hover:file:bg-raised"
            onChange={(event) => setPlan(event.currentTarget.files?.[0])}
          />
        </FormField>
        <RequestError text={failure()} />
        <div class="flex flex-wrap items-center justify-end gap-2">
          <Show when={props.floor}>
            <Show
              when={confirming()}
              fallback={
                <Button
                  type="button"
                  variant="ghost"
                  class="mr-auto text-crit"
                  onClick={() => setConfirming(true)}
                >
                  {t.delete}
                </Button>
              }
            >
              <p class="w-full text-crit text-sm">{t.confirmDelete}</p>
              <Button
                type="button"
                variant="secondary"
                class="mr-auto text-crit"
                disabled={busy()}
                onClick={() => void remove()}
              >
                {t.confirm}
              </Button>
            </Show>
          </Show>
          <Button type="button" variant="ghost" onClick={() => props.onOpenChange(false)}>
            {t.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {props.floor ? t.save : t.add}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
