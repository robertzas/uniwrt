import { For, Show } from "solid-js";
import type { Issue } from "../../../api/gen/Issue";
import type { SecretImport } from "../../../api/gen/SecretImport";
import type { SecretSource } from "../../../api/gen/SecretSource";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Card, CardHeader } from "../../../ui/Card";
import { TextField } from "../../../ui/TextField";
import { pathWords } from "../revisions/changes";

const t = strings.settings.fileImport;

const TONE: Record<SecretSource, "ok" | "neutral" | "warn"> = {
  file: "ok",
  kept: "neutral",
  missing: "warn",
  generated: "neutral",
};

/**
 * Each secret of the imported document and where its value comes from (the file, the site's
 * own at the same path, a VPN key made anew on accept, or nowhere), with a field for each
 * missing one; its error, if the
 * document needs it or the accept refused the value, under the field.
 */
export function FileSecrets(props: {
  secrets: readonly SecretImport[];
  values: Readonly<Record<string, string>>;
  onValue: (path: string, value: string) => void;
  errors: ReadonlyMap<string, Issue>;
  nodeName: (id: number) => string | undefined;
  disabled: boolean;
}) {
  const words = (path: string) => pathWords(path.split("/"), props.nodeName).join(" › ");
  return (
    <Card aria-label={t.secrets}>
      <CardHeader title={t.secrets} description={t.secretsHint} />
      <Show
        when={props.secrets.length > 0}
        fallback={<p class="px-4 pb-4 text-fg-muted text-sm">{t.noSecretsIn}</p>}
      >
        <ul class="border-edge/60 border-t">
          <For each={props.secrets}>
            {(secret) => (
              <li class="flex flex-col gap-2 border-edge/60 border-b px-4 py-2.5 last:border-b-0">
                <div class="flex items-center justify-between gap-3">
                  <span class="min-w-0 break-words font-mono text-fg text-xs">
                    {words(secret.path)}
                  </span>
                  <Badge tone={TONE[secret.source]}>{t.source[secret.source]}</Badge>
                </div>
                <Show when={secret.source === "missing"}>
                  <TextField
                    label={t.value(words(secret.path))}
                    name={`secret-${secret.path}`}
                    type="password"
                    autocomplete="off"
                    value={props.values[secret.path] ?? ""}
                    onInput={(value) => props.onValue(secret.path, value)}
                    hint={t.missingHint}
                    error={props.errors.get(secret.path)?.message}
                    disabled={props.disabled}
                  />
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </Card>
  );
}
