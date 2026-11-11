import { Show } from "solid-js";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { TextField } from "../../ui/TextField";
import { type AddressDraft, type AddressErrors, mainNetwork, moves } from "./plan";

const t = strings.setup.addresses;

/**
 * The wizard's address plan (network.md, Address plan; D-077, D-082): the site's space and
 * the router's address; the main network is the /24 around it. When that moves the router,
 * the step says how the apply carries the network over (D-081).
 */
export function AddressStep(props: {
  draft: AddressDraft;
  errors: AddressErrors;
  /** Where the router is now. */
  host: string;
  domain: string;
  onChange: (next: Partial<AddressDraft>) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const ok = () => Object.keys(props.errors).length === 0;
  const lan = () => mainNetwork(props.draft);
  return (
    <div class="flex flex-col gap-5">
      <div>
        <h2 class="font-semibold text-lg tracking-tight">{t.heading}</h2>
        <p class="mt-1.5 text-fg-muted text-sm">{t.intro}</p>
      </div>
      <div class="grid gap-4 sm:grid-cols-2">
        <TextField
          label={t.space}
          name="address-space"
          value={props.draft.space}
          onInput={(space) => props.onChange({ space })}
          hint={t.spaceHint}
          error={props.errors.space}
        />
        <TextField
          label={t.router}
          name="router-address"
          value={props.draft.router}
          onInput={(router) => props.onChange({ router })}
          hint={t.routerHint}
          error={props.errors.router}
        />
      </div>
      <Show when={ok()}>
        <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 rounded-control border border-edge px-4 py-3 text-sm">
          <dt class="text-fg-muted">{t.main}</dt>
          <dd class="font-mono">{lan().subnet}</dd>
          <dt class="text-fg-muted">{t.pool}</dt>
          <dd class="font-mono">{t.range(lan().start, lan().end)}</dd>
          <dt class="text-fg-muted">{t.domain}</dt>
          <dd class="font-mono">{props.domain}</dd>
        </dl>
        <p class="text-fg-muted text-sm">
          {moves(props.host, props.draft)
            ? t.moves(props.host, lan().address.split("/")[0] ?? "")
            : t.stays}
        </p>
      </Show>
      <div class="flex flex-wrap justify-end gap-2">
        <Button variant="ghost" onClick={props.onBack}>
          {strings.setup.back}
        </Button>
        <Button variant="primary" disabled={!ok()} onClick={props.onNext}>
          {strings.setup.next}
        </Button>
      </div>
    </div>
  );
}
