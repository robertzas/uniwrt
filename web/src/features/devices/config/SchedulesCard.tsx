import { useQuery } from "@tanstack/solid-query";
import { createSignal, onCleanup, Show, useContext } from "solid-js";
import type { EventRecord } from "../../../api/gen/EventRecord";
import type { Node } from "../../../api/gen/Node";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { NodeReboot } from "../../../api/gen/NodeReboot";
import type { SiteView } from "../../../api/gen/SiteView";
import { api, errorText } from "../../../lib/api";
import { formatDateTime } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { nextTime, spanText, timeOf, timeText, zonedText } from "../../../lib/schedule";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { Segmented } from "../../../ui/Segmented";
import { WeeklyTimeField } from "../../../ui/WeeklyTimeField";
import {
  entryOf,
  type NodeSchedulesDraft,
  nodeSchedulesDraft,
  rebootOf,
  withSchedules,
} from "./schedules";

const t = strings.device.schedules;
const KINDS: readonly NodeReboot["kind"][] = ["site", "own", "never"];
/** How far back a missed restart or an LED refusal is still worth showing. */
const RECENT_MS = 8 * 24 * 3600 * 1000;
const KIND_FILTER = ["scheduled_reboot_missed", "led_schedule_failed", "led_schedule_ok"];

/**
 * A device's schedules (network.md, Scheduled restarts and LEDs schedule; D-138): whether it
 * restarts with the site, never or at its own time, and whether its LEDs go dark with the site's
 * schedule; with the next restart, a recently missed one, its LEDs now and a refusal to follow.
 */
export function SchedulesCard(props: { node: Node; facts: NodeFacts | null; admin: boolean }) {
  const live = useContext(LiveContext);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  return (
    <Card aria-label={t.title}>
      <Show
        when={site.data}
        fallback={
          <>
            <CardHeader title={t.title} />
            <div class="px-4 pb-4">
              <RequestError text={site.error?.message} />
            </div>
          </>
        }
      >
        {(view) => (
          <SchedulesForm view={view()} node={props.node} facts={props.facts} admin={props.admin} />
        )}
      </Show>
    </Card>
  );
}

function SchedulesForm(props: {
  view: SiteView;
  node: Node;
  facts: NodeFacts | null;
  admin: boolean;
}) {
  const stored = () =>
    nodeSchedulesDraft(
      entryOf(props.view.config, props.node.id),
      props.view.config.schedules.reboot,
    );
  const [edits, setEdits] = createSignal<Partial<NodeSchedulesDraft>>({});
  const draft = (): NodeSchedulesDraft => ({ ...stored(), ...edits() });
  const [error, setError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const [now, setNow] = createSignal(new Date());
  const timer = setInterval(() => setNow(new Date()), 60_000);
  onCleanup(() => clearInterval(timer));

  const events = useQuery(() => ({
    queryKey: ["events", "schedules", props.node.id],
    queryFn: ({ signal }) =>
      api.events.page(
        {
          categories: [],
          kinds: KIND_FILTER,
          min: null,
          nodes: [props.node.id],
          clients: [],
          from: Date.now() - RECENT_MS,
          to: null,
        },
        null,
        20,
        signal,
      ),
    refetchInterval: 60_000,
  }));
  /** The newest event of `kinds` among the recent ones (newest first). */
  const latest = (...kinds: string[]): EventRecord | undefined =>
    (events.data?.events ?? []).find((event) => kinds.includes(event.what.kind));

  const zone = () => props.view.config.site.timezone;
  const schedules = () => props.view.config.schedules;
  const managed = () => props.node.mode === "managed";
  const dirty = () => JSON.stringify(draft()) !== JSON.stringify(stored());
  const change = (next: Partial<NodeSchedulesDraft>) => {
    setEdits((was) => ({ ...was, ...next }));
    setError(undefined);
    setSaved(undefined);
  };

  /** The time that restarts the device as drafted, if any. */
  const time = () => {
    const now = draft();
    if (now.reboot === "never") return undefined;
    if (now.reboot === "site") return schedules().reboot ?? undefined;
    const own = timeOf(now.own);
    return typeof own === "string" ? undefined : own;
  };
  const next = () => {
    const at = time();
    const when = at && nextTime(at, zone(), now());
    return when ? t.next(zonedText(when, zone())) : t.noNext;
  };
  const missed = () => {
    const event = latest("scheduled_reboot_missed");
    if (event?.what.kind !== "scheduled_reboot_missed") return undefined;
    return t.missed(formatDateTime(event.at), strings.events.detail.rebootMissed[event.what.why]);
  };
  const ledFailure = () => {
    const event = latest("led_schedule_failed", "led_schedule_ok");
    return event?.what.kind === "led_schedule_failed" ? event.what.error : undefined;
  };
  const leds = () => props.facts?.system.leds ?? null;

  const save = async () => {
    const reboot = rebootOf(draft());
    if (typeof reboot === "string") {
      setError(reboot);
      return;
    }
    const ledSchedule = draft().leds;
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => withSchedules(config, props.node.id, reboot, ledSchedule),
        t.message(props.node.name),
      );
      setEdits({});
      setSaved(view.revision.id);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <CardHeader title={t.title} description={t.description(zone())} />
      <form
        class="flex flex-col gap-4 px-4 pb-4"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <Show when={!managed()}>
          <p class="text-fg-muted text-sm">{t.observed}</p>
        </Show>
        <section class="flex flex-col gap-2" aria-label={t.reboot}>
          <h3 class="font-semibold text-sm">{t.reboot}</h3>
          <Segmented
            label={t.reboot}
            value={draft().reboot}
            options={KINDS.map((kind) => ({ value: kind, label: t.rebootKinds[kind] }))}
            onChange={(reboot) => change({ reboot })}
            disabled={!props.admin}
            class="self-start"
          />
          <Show when={draft().reboot === "site"}>
            <p class="text-fg-muted text-xs">
              <Show when={schedules().reboot} fallback={t.siteNone}>
                {(site) => t.siteTime(timeText(site()))}
              </Show>
            </p>
          </Show>
          <Show when={draft().reboot === "own"}>
            <WeeklyTimeField
              value={draft().own}
              onChange={(own) => change({ own })}
              error={error()}
              disabled={!props.admin}
            />
          </Show>
          <Show when={managed()}>
            <p class="text-sm">{next()}</p>
          </Show>
          <Show when={missed()}>{(text) => <p class="text-warn text-xs">{text()}</p>}</Show>
        </section>
        <section class="flex flex-col gap-2" aria-label={t.leds}>
          <h3 class="font-semibold text-sm">{t.leds}</h3>
          <Checkbox
            checked={draft().leds}
            onChange={(leds) => change({ leds })}
            label={t.ledsFollow}
            hint={
              schedules().leds_off
                ? t.ledsSite(schedules().leds_off?.spans.map(spanText).join(" · ") ?? "")
                : t.ledsNone
            }
            disabled={!props.admin}
          />
          <Show when={leds()}>
            {(now) => (
              <p class="text-fg-muted text-xs">
                {t.ledsNow(now().lit, now().count)}
                <Show when={now().held}> {t.ledsHeld}</Show>
              </p>
            )}
          </Show>
          <Show when={ledFailure()}>
            {(text) => (
              <p class="text-crit text-xs" role="alert">
                {t.ledsFailed(text())}
              </p>
            )}
          </Show>
        </section>
        <RequestError text={failure()} />
        <Show when={saved()}>
          {(revision) => <SavedRevision revision={revision()} note={t.saved} />}
        </Show>
        <Show when={props.admin && dirty()}>
          <div class="flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setEdits({});
                setError(undefined);
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
      </form>
    </>
  );
}
