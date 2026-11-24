import type { JSX } from "@solidjs/web";
import { Plus, Trash2 } from "lucide";
import { createSignal, For, Show } from "solid-js";
import { strings } from "../../../../lib/strings";
import { Button } from "../../../../ui/Button";
import { Checkbox } from "../../../../ui/Checkbox";
import { Segmented } from "../../../../ui/Segmented";
import { TextField } from "../../../../ui/TextField";
import type { UpstreamErrors } from "./check";
import {
  moveResolver,
  type Protocol,
  presetResolver,
  type ResolverDraft,
  type UpstreamDraft,
  withResolver,
} from "./draft";
import { MAX, PRESET_IDS } from "./presets";
import { ResolverRow } from "./ResolverRow";

const t = strings.networks.upstream;
const PROTOCOLS: readonly Protocol[] = ["dot", "doh"];

type Change = (next: (now: UpstreamDraft) => Partial<UpstreamDraft>) => void;

/**
 * The layers in the order dnsmasq asks them: the encrypted resolvers (reorderable, the DoT ones
 * kept together), the plain servers, the ISP's, and the switch that keeps queries encrypted.
 */
export function UpstreamEditor(props: {
  draft: UpstreamDraft;
  errors: UpstreamErrors;
  change: Change;
}) {
  const [protocol, setProtocol] = createSignal<Protocol>("dot");
  const strict = () => props.draft.strict && props.draft.encrypted.length > 0;
  const setRow = (index: number, row: ResolverDraft) =>
    props.change((now) => ({ encrypted: now.encrypted.map((r, i) => (i === index ? row : r)) }));
  const setServer = (index: number, value: string) =>
    props.change((now) => ({ servers: now.servers.map((s, i) => (i === index ? value : s)) }));
  const taken = (preset: string) =>
    preset !== "custom" &&
    props.draft.encrypted.some((r) => r.protocol === protocol() && r.preset === preset);

  return (
    <>
      <Layer title={t.encrypted} hint={t.encryptedHint} error={props.errors.encrypted}>
        <For each={props.draft.encrypted} keyed={false}>
          {(row, i) => (
            <ResolverRow
              row={row()}
              index={i}
              first={i === 0}
              last={i === props.draft.encrypted.length - 1}
              errors={props.errors.resolvers[i] ?? {}}
              onChange={(next) => setRow(i, next)}
              onMove={(by) =>
                props.change((now) => ({ encrypted: moveResolver(now.encrypted, i, by) }))
              }
              onRemove={() =>
                props.change((now) => ({
                  encrypted: now.encrypted.filter((_, j) => j !== i),
                  strict: now.encrypted.length > 1 && now.strict,
                }))
              }
            />
          )}
        </For>
        <fieldset class="flex flex-wrap items-center gap-2" aria-label={t.addResolver}>
          <span class="text-fg-muted text-xs">{t.addResolver}:</span>
          <Segmented
            label={t.addProtocol}
            value={protocol()}
            options={PROTOCOLS.map((value) => ({ value, label: t.protocols[value] ?? value }))}
            onChange={setProtocol}
          />
          <For each={[...PRESET_IDS, "custom" as const]}>
            {(preset) => (
              <Button
                size="sm"
                icon={Plus}
                disabled={props.draft.encrypted.length >= MAX || taken(preset)}
                onClick={() =>
                  props.change((now) => ({
                    encrypted: withResolver(now.encrypted, presetResolver(protocol(), preset)),
                  }))
                }
              >
                {t.presets[preset]}
              </Button>
            )}
          </For>
        </fieldset>
      </Layer>

      <Layer
        title={t.servers}
        hint={strict() ? t.leftOut : t.serversHint}
        error={props.errors.serverList}
      >
        <Show when={props.draft.servers.length > 0}>
          <div class="grid gap-3 sm:grid-cols-2">
            <For each={props.draft.servers} keyed={false}>
              {(server, i) => (
                <div class="flex items-start gap-1">
                  <div class="min-w-0 flex-1">
                    <TextField
                      label={t.server(i + 1)}
                      value={server()}
                      error={props.errors.servers[i]}
                      onInput={(value) => setServer(i, value)}
                    />
                  </div>
                  <Button
                    class="mt-7"
                    variant="ghost"
                    icon={Trash2}
                    aria-label={t.removeServer(i + 1)}
                    onClick={() =>
                      props.change((d) => ({ servers: d.servers.filter((_, j) => j !== i) }))
                    }
                  />
                </div>
              )}
            </For>
          </div>
        </Show>
        <Button
          class="self-start"
          size="sm"
          icon={Plus}
          disabled={props.draft.servers.length >= MAX}
          onClick={() => props.change((d) => ({ servers: [...d.servers, ""] }))}
        >
          {t.addServer}
        </Button>
      </Layer>

      <div class="flex flex-col gap-3">
        <Checkbox
          label={t.ispOption}
          hint={strict() ? t.leftOut : t.ispHint}
          checked={props.draft.isp}
          onChange={(isp) => props.change(() => ({ isp }))}
        />
        <Checkbox
          label={t.strict}
          hint={props.draft.encrypted.length === 0 ? t.strictOff : t.strictHint}
          checked={strict()}
          disabled={props.draft.encrypted.length === 0}
          onChange={(on) => props.change(() => ({ strict: on }))}
        />
        <Show when={props.errors.strict ?? props.errors.layers}>
          {(text) => <p class="text-crit text-xs">{text()}</p>}
        </Show>
      </div>
    </>
  );
}

/** One layer's section: its title, what it does, its rows. */
function Layer(props: {
  title: string;
  hint: string;
  error: string | undefined;
  children: JSX.Element;
}) {
  return (
    <fieldset class="flex flex-col gap-3" aria-label={props.title}>
      <div>
        <p class="font-medium text-sm">{props.title}</p>
        <p class="text-fg-muted text-xs">{props.hint}</p>
      </div>
      {props.children}
      <Show when={props.error}>{(text) => <p class="text-crit text-xs">{text()}</p>}</Show>
    </fieldset>
  );
}
