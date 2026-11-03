import { KeyRound, Undo2 } from "lucide";
import { createUniqueId, Match, Switch } from "solid-js";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { FormField, INPUT } from "../../../ui/FormField";
import { Icon } from "../../../ui/Icon";
import type { Secret } from "./draft";

const t = strings.settings.notifications.secret;

/**
 * A secret the server never shows again: one stored reads as set (a URL as its scheme and host)
 * and is kept, replaced or removed; a new one is typed in.
 */
export function SecretField(props: {
  label: string;
  value: Secret;
  /** What a stored one shows: `true` for "Set", or a URL's scheme and host. */
  stored: string | boolean;
  onChange: (value: Secret) => void;
  /** Whether a stored one can be removed (one the kind needs is only replaced). */
  removable?: boolean;
  /** A URL is shown as typed; tokens and passwords are hidden. */
  plain?: boolean;
  placeholder?: string;
  hint?: string;
  error?: string | undefined;
}) {
  const id = createUniqueId();
  const isStored = () => props.stored !== false;
  const shown = () => (typeof props.stored === "string" ? `${props.stored}/…` : t.set);
  const box =
    "flex h-9 min-w-0 flex-1 items-center gap-2 rounded-control border border-edge bg-raised px-2.5 text-sm";

  return (
    <FormField label={props.label} for={id} hint={props.hint} error={props.error}>
      <div class="flex items-center gap-2">
        <Switch>
          <Match when={props.value.mode === "keep"}>
            <div class={box}>
              <Icon icon={KeyRound} class="size-3.5 shrink-0 text-fg-subtle" />
              <span id={id} class="truncate text-fg-muted">
                {shown()}
              </span>
            </div>
            <Button
              type="button"
              size="sm"
              variant="secondary"
              aria-label={`${t.replace}: ${props.label}`}
              onClick={() => props.onChange({ mode: "set", value: "" })}
            >
              {t.replace}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              hidden={!props.removable}
              aria-label={`${t.remove}: ${props.label}`}
              onClick={() => props.onChange({ mode: "remove" })}
            >
              {t.remove}
            </Button>
          </Match>
          <Match when={props.value.mode === "remove"}>
            <div class={[box, "border-dashed bg-transparent"]}>
              <span id={id} class="truncate text-fg-subtle">
                {t.removed}
              </span>
            </div>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              icon={Undo2}
              onClick={() => props.onChange({ mode: "keep" })}
            >
              {t.undo}
            </Button>
          </Match>
          <Match when={props.value.mode === "set"}>
            <input
              id={id}
              type={props.plain ? "text" : "password"}
              value={props.value.mode === "set" ? props.value.value : ""}
              onInput={(event) => props.onChange({ mode: "set", value: event.currentTarget.value })}
              placeholder={isStored() ? t.newValue : props.placeholder}
              autocomplete={props.plain ? "off" : "new-password"}
              autocapitalize="none"
              spellcheck={false}
              aria-invalid={props.error ? "true" : undefined}
              class={[INPUT, "flex-1 font-mono", props.error ? "border-crit" : "border-edge"]}
            />
            <Button
              type="button"
              size="sm"
              variant="ghost"
              icon={Undo2}
              hidden={!isStored()}
              onClick={() => props.onChange({ mode: "keep" })}
            >
              {t.keep}
            </Button>
          </Match>
        </Switch>
      </div>
    </FormField>
  );
}
