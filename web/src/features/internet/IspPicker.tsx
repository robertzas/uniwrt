import { Search } from "lucide";
import { createMemo, createSignal, createUniqueId, For, Show } from "solid-js";
import type { Preset } from "../../api/gen/Preset";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { FormField, INPUT } from "../../ui/FormField";
import { Icon } from "../../ui/Icon";
import { countriesOf, offered } from "./summary";

const w = strings.internet.wizard;

const TONE: Record<Preset["status"], Tone> = {
  verified: "ok",
  community: "info",
  unverified: "warn",
};

/** Country → ISP (wan.md, Wizard flow): the country's ISPs, searchable, and the generic ones. */
export function IspPicker(props: {
  presets: readonly Preset[];
  /** The site's country, which the list starts with. */
  country: string | null;
  chosen: string | null;
  onPick: (preset: Preset) => void;
}) {
  const countries = createMemo(() => countriesOf(props.presets));
  const known = (code: string | null) => countries().some((c) => c.code === code);
  const [country, setCountry] = createSignal<string | null>(
    known(props.country) ? props.country : null,
  );
  const [search, setSearch] = createSignal("");
  const list = createMemo(() => offered(props.presets, country(), search()));
  const countryId = createUniqueId();
  const searchId = createUniqueId();

  return (
    <div class="flex flex-col gap-4">
      <div class="grid gap-3 sm:grid-cols-[minmax(0,14rem)_1fr]">
        <FormField label={w.country} for={countryId}>
          <select
            id={countryId}
            value={country() ?? ""}
            onChange={(event) => setCountry(event.currentTarget.value || null)}
            class={[INPUT, "border-edge"]}
          >
            <option value="">{w.anyCountry}</option>
            <For each={countries()}>{(c) => <option value={c.code}>{c.name}</option>}</For>
          </select>
        </FormField>
        <FormField label={w.search} for={searchId}>
          <div class="relative">
            <Icon
              icon={Search}
              class="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2.5 size-4 text-fg-subtle"
            />
            <input
              id={searchId}
              type="search"
              value={search()}
              onInput={(event) => setSearch(event.currentTarget.value)}
              autocomplete="off"
              class={[INPUT, "border-edge pl-8"]}
            />
          </div>
        </FormField>
      </div>
      <Show
        when={list().isps.length + list().generic.length > 0}
        fallback={<p class="text-fg-muted text-sm">{w.noMatch}</p>}
      >
        <div class="flex flex-col gap-3">
          <Show when={list().isps.length > 0}>
            <Group
              label={countries().find((c) => c.code === country())?.name ?? ""}
              presets={list().isps}
              chosen={props.chosen}
              onPick={props.onPick}
            />
          </Show>
          <Show when={list().generic.length > 0}>
            <Group
              label={w.generic}
              presets={list().generic}
              chosen={props.chosen}
              onPick={props.onPick}
            />
          </Show>
        </div>
      </Show>
    </div>
  );
}

function Group(props: {
  label: string;
  presets: readonly Preset[];
  chosen: string | null;
  onPick: (preset: Preset) => void;
}) {
  return (
    <section aria-label={props.label}>
      <h3 class="mb-1.5 font-medium text-fg-subtle text-xs uppercase tracking-wide">
        {props.label}
      </h3>
      <ul class="flex flex-col divide-y divide-edge overflow-hidden rounded-control border border-edge">
        <For each={props.presets}>
          {(preset) => (
            <li>
              <button
                type="button"
                class={[
                  "flex w-full items-start gap-3 px-3 py-2.5 text-left transition-colors hover:bg-raised focus-visible:bg-raised focus-visible:outline-none",
                  { "bg-accent/10": preset.id === props.chosen },
                ]}
                aria-current={preset.id === props.chosen ? "true" : undefined}
                onClick={() => props.onPick(preset)}
              >
                <div class="min-w-0 flex-1">
                  <p class="font-medium text-sm">
                    {preset.name} <span class="font-normal text-fg-muted">· {preset.variant}</span>
                  </p>
                  <Show when={preset.notes}>
                    <p class="mt-0.5 line-clamp-2 text-fg-muted text-xs">{preset.notes}</p>
                  </Show>
                </div>
                <Show when={preset.country !== null}>
                  <span title={w.statusHint[preset.status]}>
                    <Badge tone={TONE[preset.status]}>{w.status[preset.status]}</Badge>
                  </span>
                </Show>
              </button>
            </li>
          )}
        </For>
      </ul>
    </section>
  );
}
