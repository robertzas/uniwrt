import { createUniqueId, For, Show } from "solid-js";
import { strings } from "../../lib/strings";
import { FormField, INPUT } from "../../ui/FormField";
import { Segmented } from "../../ui/Segmented";
import type { DraftErrors } from "./check";
import type { Draft, Field, Ipv6Mode, Proto } from "./draft";

const t = strings.internet;

/** Fields shown in a monospaced font: addresses and codes. */
const MONO = new Set<Field>([
  "vlan",
  "pcp",
  "mtu",
  "mac",
  "clientId",
  "sendOptions",
  "address",
  "gateway",
  "dns",
  "aftr",
  "reqprefix",
  "sixrdPrefix",
  "borderRelay",
  "ipv4MaskLen",
]);

const NUMERIC = new Set<Field>(["vlan", "pcp", "mtu", "reqprefix", "ipv4MaskLen"]);

/** One field of the form, labelled, with its hint or error. */
function WanField(props: {
  field: Field;
  draft: Draft;
  errors: DraftErrors;
  storedPassword: boolean;
  onChange: (change: Partial<Draft>) => void;
}) {
  const id = createUniqueId();
  const words = () => t.fields[props.field];
  const hint = () =>
    props.field === "password" && props.storedPassword
      ? t.fields.passwordStored
      : words().hint || undefined;
  return (
    <FormField label={words().label} for={id} hint={hint()} error={props.errors[props.field]}>
      <input
        id={id}
        type={props.field === "password" ? "password" : "text"}
        value={props.draft[props.field]}
        onInput={(event) => props.onChange({ [props.field]: event.currentTarget.value })}
        inputmode={NUMERIC.has(props.field) ? "numeric" : undefined}
        autocomplete={props.field === "password" ? "new-password" : "off"}
        autocapitalize="none"
        spellcheck={false}
        aria-invalid={props.errors[props.field] ? "true" : undefined}
        class={[
          INPUT,
          { "font-mono": MONO.has(props.field) },
          props.errors[props.field] ? "border-crit" : "border-edge",
        ]}
      />
    </FormField>
  );
}

/** A grid of fields. */
export function FieldGrid(props: {
  fields: readonly Field[];
  draft: Draft;
  errors: DraftErrors;
  storedPassword: boolean;
  onChange: (change: Partial<Draft>) => void;
}) {
  return (
    <div class="grid gap-3 sm:grid-cols-2">
      <For each={props.fields}>
        {(field) => (
          <WanField
            field={field}
            draft={props.draft}
            errors={props.errors}
            storedPassword={props.storedPassword}
            onChange={props.onChange}
          />
        )}
      </For>
    </div>
  );
}

const PROTOS: readonly Proto[] = ["dhcp", "pppoe", "static", "ds_lite", "none"];
const IPV6: readonly Ipv6Mode[] = ["dhcpv6", "6rd", "off"];

/** Every setting: how IPv4 and IPv6 arrive, then the fields they take. */
export function AdvancedFields(props: {
  draft: Draft;
  fields: readonly Field[];
  errors: DraftErrors;
  combo: string | undefined;
  storedPassword: boolean;
  onChange: (change: Partial<Draft>) => void;
}) {
  return (
    <div class="flex flex-col gap-4">
      <div class="flex flex-col gap-3 sm:flex-row sm:flex-wrap">
        <Segmented
          label={t.wizard.ipv4}
          value={props.draft.proto}
          options={PROTOS.map((value) => ({ value, label: t.proto[value] }))}
          onChange={(proto) => props.onChange({ proto })}
        />
        <Segmented
          label={t.wizard.ipv6}
          value={props.draft.ipv6}
          options={IPV6.map((value) => ({ value, label: t.ipv6Mode[value] }))}
          onChange={(ipv6) => props.onChange({ ipv6 })}
        />
      </div>
      <Show when={props.combo}>
        <p class="text-crit text-sm" role="alert">
          {props.combo}
        </p>
      </Show>
      <FieldGrid
        fields={props.fields}
        draft={props.draft}
        errors={props.errors}
        storedPassword={props.storedPassword}
        onChange={props.onChange}
      />
    </div>
  );
}
