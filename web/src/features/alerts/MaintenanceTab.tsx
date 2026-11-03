import { CalendarClock, CalendarPlus, RotateCw, Trash2 } from "lucide";
import { createMemo, createSignal, createUniqueId, For, onSettled, Show } from "solid-js";
import type { MaintenanceWindow } from "../../api/gen/MaintenanceWindow";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import { api } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { EmptyState } from "../../ui/EmptyState";
import { Segmented } from "../../ui/Segmented";
import { showToast } from "../../ui/Toast";
import {
  checkWindow,
  emptyDraft,
  isOn,
  sortWindows,
  type WindowDraft,
  type WindowErrors,
} from "./maintenance";
import { SnoozeMenu } from "./SnoozeMenu";
import { errorText, FormField, INPUT } from "./shared";

const t = strings.alerts.maintenance;

const when = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

/** The windows on now and to come, a quick snooze, and a form to schedule one (admins). */
export function MaintenanceTab(props: {
  nodes: readonly NodeSummary[];
  admin: boolean;
  now: number;
}) {
  const [windows, setWindows] = createSignal<MaintenanceWindow[] | undefined>();
  const [failed, setFailed] = createSignal(false);
  const load = (signal?: AbortSignal) => {
    setFailed(false);
    api.maintenance
      .list(signal)
      .then((list) => setWindows(list.windows))
      .catch(() => {
        if (!signal?.aborted) setFailed(true);
      });
  };
  onSettled(() => {
    const controller = new AbortController();
    load(controller.signal);
    return () => controller.abort();
  });
  // The page's clock ticks every 30 s; a snooze just made starts later than its last tick.
  const at = () => Math.max(props.now, Date.now());
  const shown = createMemo(() => sortWindows(windows() ?? [], at()));
  const nodeName = (id: number) =>
    props.nodes.find((node) => node.id === id)?.name ?? strings.events.node(id);
  const remove = (window: MaintenanceWindow) =>
    api.maintenance
      .remove(window.id)
      .then(() => setWindows((list) => list?.filter((w) => w.id !== window.id)))
      .catch((error: unknown) =>
        showToast({ title: t.changeFailed, description: errorText(error), tone: "crit" }),
      );

  return (
    <div class="grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <Card class="overflow-hidden" aria-label={t.label}>
        <CardHeader
          title={t.label}
          description={t.description}
          actions={
            <Show when={props.admin}>
              <SnoozeMenu label={t.snooze} onSnoozed={() => load()} />
            </Show>
          }
        />
        <Show
          when={windows()}
          fallback={
            <Show
              when={failed()}
              fallback={
                <p role="status" class="py-16 text-center text-fg-subtle text-sm">
                  {t.loading}…
                </p>
              }
            >
              <div role="alert" class="flex flex-col items-center gap-3 py-12 text-sm">
                <p class="text-fg-muted">{t.failed}</p>
                <Button size="sm" icon={RotateCw} onClick={() => load()}>
                  {strings.alerts.history.retry}
                </Button>
              </div>
            </Show>
          }
        >
          <Show
            when={shown().length > 0}
            fallback={
              <EmptyState icon={CalendarClock} title={t.emptyTitle} description={t.empty} />
            }
          >
            <ul class="border-edge border-t">
              <For each={shown()}>
                {(window) => {
                  const on = () => isOn(window, at());
                  return (
                    <li class="flex items-start gap-3 border-edge/60 border-b px-4 py-3 last:border-b-0">
                      <div class="flex min-w-0 flex-1 flex-col gap-1">
                        <div class="flex flex-wrap items-center gap-2">
                          <Badge tone={on() ? "warn" : "neutral"}>
                            {on() ? t.now : t.upcoming}
                          </Badge>
                          <span class="font-medium text-sm tabular-nums">
                            {on()
                              ? t.until(when.format(new Date(window.ends_at)))
                              : t.from(
                                  when.format(new Date(window.starts_at)),
                                  when.format(new Date(window.ends_at)),
                                )}
                          </span>
                        </div>
                        <p class="text-fg-muted text-xs">
                          {window.nodes.length === 0
                            ? t.every
                            : window.nodes.map(nodeName).join(", ")}
                          <Show when={window.note}>{(note) => <> · {note()}</>}</Show>
                        </p>
                      </div>
                      <Show when={props.admin}>
                        <Button
                          size="sm"
                          variant="ghost"
                          icon={Trash2}
                          title={on() ? t.endHint : t.cancelHint}
                          onClick={() => void remove(window)}
                        >
                          {on() ? t.end : t.cancel}
                        </Button>
                      </Show>
                    </li>
                  );
                }}
              </For>
            </ul>
          </Show>
        </Show>
      </Card>
      <Show when={props.admin} fallback={<p class="text-fg-subtle text-sm">{t.readOnly}</p>}>
        <ScheduleForm
          nodes={props.nodes}
          onAdded={(window) => setWindows((list) => [...(list ?? []), window])}
        />
      </Show>
    </div>
  );
}

function ScheduleForm(props: {
  nodes: readonly NodeSummary[];
  onAdded: (window: MaintenanceWindow) => void;
}) {
  const id = createUniqueId();
  const [draft, setDraft] = createSignal<WindowDraft>(emptyDraft());
  const [errors, setErrors] = createSignal<WindowErrors>({});
  const [saving, setSaving] = createSignal(false);
  const change = (next: Partial<WindowDraft>) => setDraft((now) => ({ ...now, ...next }));
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    const checked = checkWindow(draft());
    if ("errors" in checked) {
      setErrors(checked.errors);
      return;
    }
    setErrors({});
    setSaving(true);
    api.maintenance
      .create(checked.window)
      .then((window) => {
        props.onAdded(window);
        setDraft(emptyDraft());
        showToast({ title: t.added, tone: "ok" });
      })
      .catch((error: unknown) =>
        showToast({ title: t.changeFailed, description: errorText(error), tone: "crit" }),
      )
      .finally(() => setSaving(false));
  };

  return (
    <Card aria-label={t.schedule}>
      <CardHeader title={t.schedule} />
      <form onSubmit={submit} novalidate class="flex flex-col gap-4 px-4 pb-4">
        <FormField label={t.starts} for={`${id}-starts`} error={errors().starts}>
          <input
            id={`${id}-starts`}
            type="datetime-local"
            class={[INPUT, errors().starts ? "border-crit" : "border-edge"]}
            value={draft().starts}
            onInput={(event) => change({ starts: event.currentTarget.value })}
          />
        </FormField>
        <FormField label={t.ends} for={`${id}-ends`} error={errors().ends}>
          <input
            id={`${id}-ends`}
            type="datetime-local"
            class={[INPUT, errors().ends ? "border-crit" : "border-edge"]}
            value={draft().ends}
            onInput={(event) => change({ ends: event.currentTarget.value })}
          />
        </FormField>
        <div class="flex flex-col gap-2">
          <span class="font-medium text-fg text-sm">{t.devices}</span>
          <Segmented
            label={t.devices}
            value={draft().every ? "every" : "some"}
            options={[
              { value: "every", label: t.everyDevice },
              { value: "some", label: t.someDevices },
            ]}
            onChange={(value) => change({ every: value === "every" })}
            class="self-start"
          />
          <Show when={!draft().every}>
            <div class="flex max-h-40 flex-col gap-1.5 overflow-y-auto rounded-control border border-edge bg-canvas p-2.5">
              <For each={props.nodes}>
                {(node) => (
                  <Checkbox
                    label={node.name}
                    checked={draft().nodes.includes(node.id)}
                    onChange={(on) =>
                      change({
                        nodes: on
                          ? [...draft().nodes, node.id]
                          : draft().nodes.filter((n) => n !== node.id),
                      })
                    }
                  />
                )}
              </For>
            </div>
            <Show when={errors().nodes}>
              {(message) => <p class="text-crit text-xs">{message()}</p>}
            </Show>
          </Show>
        </div>
        <FormField label={t.note} for={`${id}-note`}>
          <input
            id={`${id}-note`}
            class={[INPUT, "border-edge"]}
            maxlength={200}
            value={draft().note}
            placeholder={t.notePlaceholder}
            onInput={(event) => change({ note: event.currentTarget.value })}
          />
        </FormField>
        <Button
          type="submit"
          variant="primary"
          icon={CalendarPlus}
          disabled={saving()}
          class="self-start"
        >
          {saving() ? `${t.adding}…` : t.add}
        </Button>
      </form>
    </Card>
  );
}
