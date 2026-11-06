import type { IconNode } from "lucide";
import { Cpu, FlaskConical, Server, Users } from "lucide";
import { createSignal, For } from "solid-js";
import type { Network } from "../../api/gen/Network";
import type { Purpose } from "../../api/gen/Purpose";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { TextField } from "../../ui/TextField";

const t = strings.networks;

const PURPOSES: [Exclude<Purpose, "default">, IconNode][] = [
  ["iot", Cpu],
  ["guest", Users],
  ["mgmt", Server],
  ["custom", FlaskConical],
];

/**
 * The first step of a new network: its purpose and, optionally, its VLAN; the address plan
 * (`GET v1/ipam/network`) suggests the rest, which the editor opens with.
 */
export function NewNetworkDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChosen: (network: Network) => void;
}) {
  const [purpose, setPurpose] = createSignal<Exclude<Purpose, "default">>("iot");
  const [vlan, setVlan] = createSignal("");
  const [vlanError, setVlanError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);

  const next = async () => {
    const text = vlan().trim();
    const id = text === "" ? undefined : Number(text);
    if (id !== undefined && !(Number.isInteger(id) && id >= 1 && id <= 4094)) {
      setVlanError(t.errors.vlan);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      props.onChosen(await api.ipam.network(purpose(), id));
      setVlan("");
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={t.choose.title}
      description={t.choose.description}
    >
      <form
        class="flex flex-col gap-4 px-5 pb-5"
        onSubmit={(event) => {
          event.preventDefault();
          void next();
        }}
      >
        <fieldset class="grid gap-2 sm:grid-cols-2">
          <legend class="sr-only">{t.editor.purpose}</legend>
          <For each={PURPOSES}>
            {([value, icon]) => (
              <button
                type="button"
                aria-pressed={purpose() === value ? "true" : "false"}
                onClick={() => setPurpose(value)}
                class={[
                  "flex items-start gap-3 rounded-control border p-3 text-left transition-colors",
                  purpose() === value
                    ? "border-accent bg-accent/10"
                    : "border-edge hover:bg-raised",
                ]}
              >
                <Icon icon={icon} class="mt-0.5 size-4 shrink-0 text-fg-muted" />
                <span class="min-w-0">
                  <span class="block font-medium text-sm">{t.purpose[value]}</span>
                  <span class="mt-0.5 block text-fg-muted text-xs">{t.purposeHint[value]}</span>
                </span>
              </button>
            )}
          </For>
        </fieldset>
        <TextField
          label={t.choose.vlan}
          value={vlan()}
          onInput={(value) => {
            setVlan(value);
            setVlanError(undefined);
          }}
          hint={t.choose.vlanHint}
          error={vlanError()}
        />
        <RequestError text={failure()} />
        <div class="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => props.onOpenChange(false)} disabled={busy()}>
            {t.editor.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {t.choose.next}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
