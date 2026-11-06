import { Plus, Trash2 } from "lucide";
import { For, Show } from "solid-js";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { INPUT } from "../../ui/FormField";
import type { DraftErrors, ReservationDraft } from "./draft";

const t = strings.networks.editor;

/** The pool's reservations as rows of MAC address, address and host name. */
export function ReservationsField(props: {
  reservations: readonly ReservationDraft[];
  errors: DraftErrors;
  onChange: (reservations: ReservationDraft[]) => void;
}) {
  const update = (index: number, next: Partial<ReservationDraft>) =>
    props.onChange(props.reservations.map((r, i) => (i === index ? { ...r, ...next } : r)));
  return (
    <section class="flex flex-col gap-2" aria-label={t.reservations}>
      <div>
        <h3 class="font-semibold text-sm">{t.reservations}</h3>
        <p class="mt-0.5 text-fg-muted text-xs">{t.reservationsHint}</p>
      </div>
      <Show
        when={props.reservations.length > 0}
        fallback={<p class="text-fg-subtle text-sm">{t.noReservations}</p>}
      >
        <div class="grid grid-cols-[1.2fr_1fr_1fr_auto] gap-x-2 gap-y-1.5 text-fg-muted text-xs">
          <span>{t.mac}</span>
          <span>{t.ip}</span>
          <span>{t.hostname}</span>
          <span />
          <For each={props.reservations} keyed={false}>
            {(reservation, i) => (
              <>
                <Cell
                  label={t.mac}
                  value={reservation().mac}
                  error={props.errors[`r${i}.mac`]}
                  onInput={(mac) => update(i, { mac })}
                />
                <Cell
                  label={t.ip}
                  value={reservation().ip}
                  error={props.errors[`r${i}.ip`]}
                  onInput={(ip) => update(i, { ip })}
                />
                <Cell
                  label={t.hostname}
                  value={reservation().name}
                  error={props.errors[`r${i}.name`]}
                  onInput={(name) => update(i, { name })}
                />
                <Button
                  variant="ghost"
                  icon={Trash2}
                  aria-label={`${t.remove} ${reservation().mac || reservation().ip}`}
                  onClick={() => props.onChange(props.reservations.filter((_, j) => j !== i))}
                />
              </>
            )}
          </For>
        </div>
      </Show>
      <div>
        <Button
          size="sm"
          icon={Plus}
          onClick={() => props.onChange([...props.reservations, { mac: "", ip: "", name: "" }])}
        >
          {t.addReservation}
        </Button>
      </div>
    </section>
  );
}

function Cell(props: {
  label: string;
  value: string;
  error: string | undefined;
  onInput: (value: string) => void;
}) {
  return (
    <div class="min-w-0">
      <input
        aria-label={props.label}
        aria-invalid={props.error ? "true" : undefined}
        class={[INPUT, "font-mono", props.error ? "border-crit" : "border-edge"]}
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
      />
      <Show when={props.error}>
        <p class="mt-1 text-crit text-xs">{props.error}</p>
      </Show>
    </div>
  );
}
