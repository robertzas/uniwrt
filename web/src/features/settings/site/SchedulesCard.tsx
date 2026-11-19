import { createMemo, createSignal, onCleanup, Show, useContext } from "solid-js";
import type { Schedules } from "../../../api/gen/Schedules";
import type { SiteView } from "../../../api/gen/SiteView";
import { errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import {
  nextTime,
  type SpanDraft,
  spanDrafts,
  spansOf,
  type TimeDraft,
  timeDraft,
  timeOf,
  timeText,
  WEEKDAYS,
  zonedText,
} from "../../../lib/schedule";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { ScheduleField } from "../../../ui/ScheduleField";
import { WeeklyTimeField } from "../../../ui/WeeklyTimeField";

const t = strings.settings.schedules;

/** The span a new LEDs-off schedule starts with: every night, 23:00 to 07:00. */
const nightSpan = (): SpanDraft => ({ days: [...WEEKDAYS], from: "23:00", to: "07:00" });

/** The site's schedules as typed. */
export interface SchedulesDraft {
  rebootOn: boolean;
  reboot: TimeDraft;
  ledsOn: boolean;
  spans: SpanDraft[];
}

export function schedulesDraft(schedules: Schedules): SchedulesDraft {
  return {
    rebootOn: schedules.reboot !== null,
    reboot: timeDraft(schedules.reboot),
    ledsOn: schedules.leds_off !== null,
    spans: spanDrafts(schedules.leds_off, nightSpan),
  };
}

/** The schedules a draft describes, or what is wrong with each part. */
export function schedulesOf(draft: SchedulesDraft): Schedules | { reboot?: string; leds?: string } {
  const reboot = draft.rebootOn ? timeOf(draft.reboot) : null;
  const spans = draft.ledsOn ? spansOf(draft.spans) : null;
  if (typeof reboot === "string" || typeof spans === "string") {
    return {
      ...(typeof reboot === "string" && { reboot }),
      ...(typeof spans === "string" && { leds: spans }),
    };
  }
  return { reboot, leds_off: spans && { spans } };
}

const isSchedules = (value: ReturnType<typeof schedulesOf>): value is Schedules =>
  "leds_off" in value;

/**
 * Settings › Site › Schedules (network.md, Scheduled restarts and LEDs schedule; D-138): when the
 * managed devices restart, and when their LEDs are off, site-wide; each device can leave them
 * (its Config tab). The controller runs them on the saved site: a save needs no apply.
 */
export function SchedulesCard(props: { view: SiteView; admin: boolean }) {
  const live = useContext(LiveContext);
  const stored = () => schedulesDraft(props.view.config.schedules);
  const [edits, setEdits] = createSignal<Partial<SchedulesDraft>>({});
  const draft = (): SchedulesDraft => ({ ...stored(), ...edits() });
  const [errors, setErrors] = createSignal<{ reboot?: string; leds?: string }>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [now, setNow] = createSignal(new Date());
  const timer = setInterval(() => setNow(new Date()), 60_000);
  onCleanup(() => clearInterval(timer));

  const zone = () => props.view.config.site.timezone;
  const dirty = () => JSON.stringify(draft()) !== JSON.stringify(stored());
  const change = (next: Partial<SchedulesDraft>) => {
    setEdits((was) => ({ ...was, ...next }));
    setErrors({});
    setSaved(undefined);
  };

  const managed = createMemo(() => live.fleet.nodes.filter((node) => node.mode === "managed"));
  const entry = (id: number) => props.view.config.nodes.find((n) => n.node === id);
  /** Who restarts with the site's time, and who has a time of their own. */
  const rebootFollowers = () =>
    managed().flatMap((node) => {
      const reboot = entry(node.id)?.reboot ?? { kind: "site" as const };
      if (reboot.kind === "site") return [node.name];
      if (reboot.kind === "own") return [t.ownTime(node.name, timeText(reboot))];
      return [];
    });
  const ledFollowers = () =>
    managed()
      .filter((node) => entry(node.id)?.led_schedule ?? true)
      .map((node) => node.name);
  const next = () => {
    if (!draft().rebootOn) return undefined;
    const time = timeOf(draft().reboot);
    const at = typeof time === "string" ? undefined : nextTime(time, zone(), now());
    return at && zonedText(at, zone());
  };

  const save = async () => {
    const schedules = schedulesOf(draft());
    if (!isSchedules(schedules)) {
      setErrors(schedules);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite((config) => ({ ...config, schedules }), t.message);
      setEdits({});
      setSaved(view.revision.id);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const Followers = (p: { names: string[] }) => (
    <p class="text-fg-subtle text-xs">
      {p.names.length > 0 ? t.followers(p.names.join(", ")) : t.noFollowers}
    </p>
  );

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description(zone())} />
      <form
        class="flex flex-col gap-5 px-4 pt-1 pb-4"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <section class="flex flex-col gap-3" aria-label={t.reboot}>
          <h3 class="font-semibold text-sm">{t.reboot}</h3>
          <Checkbox
            checked={draft().rebootOn}
            onChange={(rebootOn) => change({ rebootOn })}
            label={t.rebootOn}
            hint={t.rebootHint}
            disabled={!props.admin}
          />
          <Show when={draft().rebootOn}>
            <WeeklyTimeField
              value={draft().reboot}
              onChange={(reboot) => change({ reboot })}
              error={errors().reboot}
              disabled={!props.admin}
            />
            <Show when={next()}>{(when) => <p class="text-sm">{t.next(when())}</p>}</Show>
            <Followers names={rebootFollowers()} />
          </Show>
        </section>
        <div class="flex flex-col gap-3">
          <ScheduleField
            title={t.leds}
            label={t.ledsOn}
            hint={t.ledsHint}
            scheduled={draft().ledsOn}
            spans={draft().spans}
            error={errors().leds}
            onChange={(next) =>
              change({
                ...(next.scheduled !== undefined && { ledsOn: next.scheduled }),
                ...(next.spans && { spans: next.spans }),
              })
            }
            fresh={nightSpan}
            disabled={!props.admin}
          />
          <Show when={draft().ledsOn}>
            <Followers names={ledFollowers()} />
          </Show>
        </div>
        <RequestError text={failure()} />
        <Show when={saved()}>
          {(revision) => <SavedRevision revision={revision()} note={t.saved} />}
        </Show>
        <Show when={props.admin} fallback={<p class="text-fg-subtle text-xs">{t.adminOnly}</p>}>
          <Show when={dirty()}>
            <div class="flex justify-end gap-2">
              <Button
                variant="ghost"
                onClick={() => {
                  setEdits({});
                  setErrors({});
                }}
                disabled={busy()}
              >
                {t.cancel}
              </Button>
              <Button type="submit" variant="primary" disabled={busy()}>
                {t.save}
              </Button>
            </div>
          </Show>
        </Show>
      </form>
    </Card>
  );
}
