/** The editor's fields per kind: ntfy, email, and a labelled text input they share. */
import { createUniqueId, Show } from "solid-js";
import { strings } from "../../../lib/strings";
import { Checkbox } from "../../../ui/Checkbox";
import { FormField, INPUT } from "../../../ui/FormField";
import { Segmented } from "../../../ui/Segmented";
import type { Draft, DraftErrors } from "./draft";
import { MAX_RECIPIENTS, withSecurity } from "./draft";
import { SecretField } from "./SecretField";

const t = strings.settings.notifications.editor;

/** A labelled text input in the forms' style. */
export function Input(props: {
  label: string;
  value: string;
  onInput: (value: string) => void;
  error?: string | undefined;
  hint?: string;
  placeholder?: string;
  mono?: boolean;
  inputmode?: "numeric" | "email" | "url";
  autocomplete?: string;
  class?: string;
}) {
  const id = createUniqueId();
  return (
    <FormField
      label={props.label}
      for={id}
      hint={props.hint}
      error={props.error}
      class={props.class}
    >
      <input
        id={id}
        type="text"
        value={props.value}
        onInput={(event) => props.onInput(event.currentTarget.value)}
        placeholder={props.placeholder}
        inputmode={props.inputmode}
        autocomplete={props.autocomplete ?? "off"}
        autocapitalize="none"
        spellcheck={false}
        aria-invalid={props.error ? "true" : undefined}
        class={[INPUT, { "font-mono": props.mono }, props.error ? "border-crit" : "border-edge"]}
      />
    </FormField>
  );
}

interface FieldsProps {
  draft: Draft;
  errors: DraftErrors;
  /** Which secrets the stored notifier has; nothing for a new one. */
  stored: { token: boolean; password: boolean };
  onChange: (change: Partial<Draft>) => void;
}

/** An ntfy topic on a server, with an access token or a username and password. */
export function NtfyFields(props: FieldsProps) {
  const d = () => props.draft;
  return (
    <div class="grid gap-4 sm:grid-cols-2">
      <Input
        label={t.server}
        value={d().server}
        onInput={(server) => props.onChange({ server })}
        error={props.errors.server}
        hint={t.serverHint}
        inputmode="url"
      />
      <Input
        label={t.topic}
        value={d().topic}
        onInput={(topic) => props.onChange({ topic })}
        error={props.errors.topic}
        hint={t.topicHint}
        mono
      />
      <div class="flex flex-col gap-1.5 sm:col-span-2">
        <span class="font-medium text-fg text-sm">{t.auth}</span>
        <Segmented
          label={t.auth}
          value={d().ntfyAuth}
          options={(["none", "token", "password"] as const).map((value) => ({
            value,
            label: t.ntfyAuth[value],
          }))}
          onChange={(ntfyAuth: Draft["ntfyAuth"]) => props.onChange({ ntfyAuth })}
          class="self-start"
        />
        <p class="text-fg-subtle text-xs">{t.authHint}</p>
      </div>
      <Show when={d().ntfyAuth === "token"}>
        <SecretField
          label={t.token}
          value={d().token}
          stored={props.stored.token}
          onChange={(token) => props.onChange({ token })}
          placeholder="tk_…"
          error={props.errors.token}
        />
      </Show>
      <Show when={d().ntfyAuth === "password"}>
        <Input
          label={t.username}
          value={d().username}
          onInput={(username) => props.onChange({ username })}
          error={props.errors.username}
        />
        <SecretField
          label={t.password}
          value={d().password}
          stored={props.stored.password}
          onChange={(password) => props.onChange({ password })}
          error={props.errors.password}
        />
      </Show>
    </div>
  );
}

/** Email over SMTP, secured by STARTTLS or TLS (never in the clear), to 1–10 addresses. */
export function EmailFields(props: FieldsProps) {
  const d = () => props.draft;
  return (
    <div class="grid gap-4 sm:grid-cols-[1fr_7rem]">
      <Input
        label={t.host}
        value={d().host}
        onInput={(host) => props.onChange({ host })}
        error={props.errors.host}
        placeholder="smtp.example.com"
      />
      <Input
        label={t.port}
        value={d().port}
        onInput={(port) => props.onChange({ port })}
        error={props.errors.port}
        inputmode="numeric"
        mono
      />
      <div class="flex flex-col gap-1.5 sm:col-span-2">
        <span class="font-medium text-fg text-sm">{t.security}</span>
        <Segmented
          label={t.security}
          value={d().security}
          options={(["starttls", "tls"] as const).map((value) => ({
            value,
            label: t.securities[value],
          }))}
          onChange={(security) => props.onChange(withSecurity(d(), security))}
          class="self-start"
        />
        <p class="text-fg-subtle text-xs">{t.securityHint[d().security]}</p>
      </div>
      <Checkbox
        label={t.emailAuth}
        checked={d().emailAuth}
        onChange={(emailAuth) => props.onChange({ emailAuth })}
        class="sm:col-span-2"
      />
      <Show when={d().emailAuth}>
        <div class="grid gap-4 sm:col-span-2 sm:grid-cols-2">
          <Input
            label={t.username}
            value={d().username}
            onInput={(username) => props.onChange({ username })}
            error={props.errors.username}
          />
          <SecretField
            label={t.password}
            value={d().password}
            stored={props.stored.password}
            onChange={(password) => props.onChange({ password })}
            error={props.errors.password}
          />
        </div>
      </Show>
      <Input
        label={t.from}
        value={d().from}
        onInput={(from) => props.onChange({ from })}
        error={props.errors.from}
        hint={t.fromHint}
        placeholder="uniwrt <uniwrt@example.com>"
        inputmode="email"
        class="sm:col-span-2"
      />
      <Input
        label={t.to}
        value={d().to}
        onInput={(to) => props.onChange({ to })}
        error={props.errors.to}
        hint={t.toHint(MAX_RECIPIENTS)}
        placeholder="you@example.com"
        inputmode="email"
        class="sm:col-span-2"
      />
    </div>
  );
}
