import { ArrowDown, ArrowUp, Trash2 } from "lucide";
import { Match, Show, Switch } from "solid-js";
import { strings } from "../../../../lib/strings";
import { Button } from "../../../../ui/Button";
import { TextField } from "../../../../ui/TextField";
import type { ResolverErrors } from "./check";
import { type DohDraft, type DotDraft, type ResolverDraft, rowName } from "./draft";

const t = strings.networks.upstream;

/** An encrypted resolver's row: its fields, and buttons that move or remove it. */
export function ResolverRow(props: {
  row: ResolverDraft;
  index: number;
  first: boolean;
  last: boolean;
  errors: ResolverErrors;
  onChange: (row: ResolverDraft) => void;
  onMove: (by: -1 | 1) => void;
  onRemove: () => void;
}) {
  const name = () => rowName(props.row, props.index);
  const doh = () => (props.row.protocol === "doh" ? props.row : undefined);
  const dot = () => (props.row.protocol === "dot" ? props.row : undefined);
  return (
    <fieldset
      class="flex flex-col gap-3 rounded-control border border-edge p-3"
      aria-label={name()}
    >
      <div class="flex items-center gap-1">
        <p class="flex-1 font-medium text-sm">
          <span class="text-fg-subtle tabular-nums">{props.index + 1}. </span>
          {name()}
        </p>
        <Button
          size="sm"
          variant="ghost"
          icon={ArrowUp}
          aria-label={t.moveUp(name())}
          disabled={props.first}
          onClick={() => props.onMove(-1)}
        />
        <Button
          size="sm"
          variant="ghost"
          icon={ArrowDown}
          aria-label={t.moveDown(name())}
          disabled={props.last}
          onClick={() => props.onMove(1)}
        />
        <Button
          size="sm"
          variant="ghost"
          icon={Trash2}
          aria-label={t.removeResolver(name())}
          onClick={() => props.onRemove()}
        />
      </div>
      <Show when={props.errors.row}>{(text) => <p class="text-crit text-xs">{text()}</p>}</Show>
      <div class="grid gap-3 sm:grid-cols-2">
        <Switch>
          <Match when={doh()}>
            {(row) => <DohFields row={row()} errors={props.errors} onChange={props.onChange} />}
          </Match>
          <Match when={dot()}>
            {(row) => <DotFields row={row()} errors={props.errors} onChange={props.onChange} />}
          </Match>
        </Switch>
      </div>
    </fieldset>
  );
}

function DohFields(props: {
  row: DohDraft;
  errors: ResolverErrors;
  onChange: (row: DohDraft) => void;
}) {
  return (
    <>
      <Switch fallback={<Fixed label={t.url} text={props.row.url} error={props.errors.url} />}>
        <Match when={props.row.preset === "custom"}>
          <TextField
            label={t.url}
            value={props.row.url}
            hint={t.urlHint}
            error={props.errors.url}
            onInput={(url) => props.onChange({ ...props.row, url })}
          />
        </Match>
        <Match when={props.row.preset === "nextdns"}>
          <TextField
            label={t.profile}
            value={props.row.profile}
            hint={t.profileHint}
            error={props.errors.url}
            onInput={(profile) => props.onChange({ ...props.row, profile })}
          />
        </Match>
      </Switch>
      <TextField
        label={t.bootstrap}
        value={props.row.bootstrap}
        hint={t.bootstrapHint}
        error={props.errors.bootstrap}
        onInput={(bootstrap) => props.onChange({ ...props.row, bootstrap })}
      />
    </>
  );
}

function DotFields(props: {
  row: DotDraft;
  errors: ResolverErrors;
  onChange: (row: DotDraft) => void;
}) {
  return (
    <>
      <Switch fallback={<Fixed label={t.name} text={props.row.name} error={props.errors.name} />}>
        <Match when={props.row.preset === "custom"}>
          <TextField
            label={t.name}
            value={props.row.name}
            hint={t.nameHint}
            error={props.errors.name}
            onInput={(name) => props.onChange({ ...props.row, name })}
          />
        </Match>
        <Match when={props.row.preset === "nextdns"}>
          <TextField
            label={t.profile}
            value={props.row.profile}
            hint={t.profileHint}
            error={props.errors.name}
            onInput={(profile) => props.onChange({ ...props.row, profile })}
          />
        </Match>
      </Switch>
      <TextField
        label={t.addresses}
        value={props.row.addresses}
        hint={t.addressesHint}
        error={props.errors.addresses}
        onInput={(addresses) => props.onChange({ ...props.row, addresses })}
      />
      <Show when={props.row.preset === "custom" || props.errors.port}>
        <TextField
          label={t.port}
          value={props.row.port}
          hint={t.portHint}
          error={props.errors.port}
          onInput={(port) => props.onChange({ ...props.row, port })}
        />
      </Show>
      <TextField
        label={t.spki}
        value={props.row.spki}
        hint={t.spkiHint}
        error={props.errors.spki}
        onInput={(spki) => props.onChange({ ...props.row, spki })}
      />
    </>
  );
}

/** A preset's value, which its row doesn't edit. */
function Fixed(props: { label: string; text: string; error: string | undefined }) {
  return (
    <div class="flex flex-col gap-1.5">
      <span class="font-medium text-sm">{props.label}</span>
      <span class="flex h-10 items-center truncate font-mono text-fg-muted text-xs">
        {props.text}
      </span>
      <Show when={props.error}>
        <p class="text-crit text-xs">{props.error}</p>
      </Show>
    </div>
  );
}
