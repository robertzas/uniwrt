import { useQuery } from "@tanstack/solid-query";
import { createMemo, createSignal, For, Show, useContext } from "solid-js";
import type { SiteView } from "../../../api/gen/SiteView";
import { ApiError, api, errorText } from "../../../lib/api";
import { countries } from "../../../lib/countries";
import { LiveContext } from "../../../lib/live";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { FormField, SELECT } from "../../../ui/FormField";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { TextField } from "../../../ui/TextField";

const t = strings.settings.site;

/** The site's settings as typed. */
export interface SiteDraft {
  name: string;
  country: string;
  timezone: string;
}

/** The IANA zones the browser knows, with `current` among them (the server has the last word). */
export function zones(current: string): string[] {
  const known =
    typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [];
  return [...new Set(["UTC", current, ...known])].sort((a, b) => a.localeCompare(b));
}

/**
 * Settings › Site (D-083): the site's name, its country (the radios' regulatory domain) and its
 * time zone (the devices' clocks, the WLANs' schedules). A save is the site's next revision.
 */
export function SiteTab(props: { admin: boolean }) {
  const live = useContext(LiveContext);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Show
        when={site.data}
        fallback={
          <div class="px-4 pb-4">
            <Show when={!site.isError}>
              <p role="status" class="text-fg-muted text-sm">
                {t.loading}…
              </p>
            </Show>
            <RequestError text={site.error?.message} />
          </div>
        }
      >
        {(view) => <SiteForm view={view()} admin={props.admin} />}
      </Show>
    </Card>
  );
}

function SiteForm(props: { view: SiteView; admin: boolean }) {
  const stored = (): SiteDraft => ({
    name: props.view.config.site.name,
    country: props.view.config.site.country ?? "",
    timezone: props.view.config.site.timezone,
  });
  const [edits, setEdits] = createSignal<Partial<SiteDraft>>({});
  const draft = (): SiteDraft => ({ ...stored(), ...edits() });
  const [errors, setErrors] = createSignal<Partial<Record<keyof SiteDraft, string>>>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<number>();
  const list = createMemo(countries);
  const zoneList = createMemo(() => zones(stored().timezone));
  const dirty = () => {
    const now = draft();
    const was = stored();
    return now.name !== was.name || now.country !== was.country || now.timezone !== was.timezone;
  };
  const change = (next: Partial<SiteDraft>) => {
    setEdits((now) => ({ ...now, ...next }));
    setErrors({});
    setSaved(undefined);
  };

  const save = async () => {
    const now = draft();
    const name = now.name.trim();
    if (name.length === 0 || name.length > 64) {
      setErrors({ name: t.nameError });
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({
          ...config,
          site: { ...config.site, name, country: now.country || null, timezone: now.timezone },
        }),
        t.message,
      );
      setEdits({});
      setSaved(view.revision.id);
    } catch (err) {
      if (err instanceof ApiError && err.issues.length > 0) {
        const found: Partial<Record<keyof SiteDraft, string>> = {};
        for (const issue of err.issues.filter((issue) => issue.severity === "error")) {
          const field = issue.path.replace(/^site\//, "") as keyof SiteDraft;
          if (field in now) found[field] = issue.message;
        }
        setErrors(found);
        setFailure(err.message);
      } else {
        setFailure(errorText(err));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      class="flex flex-col gap-4 px-4 pt-1 pb-4"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div class="grid gap-4 sm:grid-cols-2">
        <TextField
          label={t.name}
          name="site-name"
          autocomplete="off"
          value={draft().name}
          onInput={(name) => change({ name })}
          error={errors().name}
          disabled={!props.admin}
        />
        <span class="hidden sm:block" />
        <FormField
          label={t.country}
          for="site-country"
          hint={t.countryHint}
          error={errors().country}
        >
          <select
            id="site-country"
            class={SELECT}
            value={draft().country}
            disabled={!props.admin}
            onChange={(event) => change({ country: event.currentTarget.value })}
          >
            <option value="">{t.pickCountry}</option>
            <For each={list()}>{(c) => <option value={c.code}>{c.name}</option>}</For>
          </select>
        </FormField>
        <FormField
          label={t.timezone}
          for="site-timezone"
          hint={t.timezoneHint}
          error={errors().timezone}
        >
          <select
            id="site-timezone"
            class={SELECT}
            value={draft().timezone}
            disabled={!props.admin}
            onChange={(event) => change({ timezone: event.currentTarget.value })}
          >
            <For each={zoneList()}>{(zone) => <option value={zone}>{zone}</option>}</For>
          </select>
        </FormField>
      </div>
      <RequestError text={failure()} />
      <Show when={saved()}>{(revision) => <SavedRevision revision={revision()} />}</Show>
      <Show when={props.admin} fallback={<p class="text-fg-subtle text-xs">{t.adminOnly}</p>}>
        <div class="flex justify-end">
          <Button type="submit" variant="primary" disabled={busy() || !dirty()}>
            {t.save}
          </Button>
        </div>
      </Show>
    </form>
  );
}
