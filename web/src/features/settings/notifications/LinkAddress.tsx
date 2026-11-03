import { Link2 } from "lucide";
import { createSignal, createUniqueId, Show, untrack } from "solid-js";
import type { NotificationSettings } from "../../../api/gen/NotificationSettings";
import { api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { FormField, INPUT } from "../../../ui/FormField";
import { showToast } from "../../../ui/Toast";
import { urlProblem } from "./draft";

const t = strings.settings.notifications.link;

/** The address as the server keeps it: no white space or trailing slash. */
const tidy = (text: string) => text.trim().replace(/\/+$/, "");

/**
 * The controller's address as people open it, which messages link to alerts at. Prefilled from
 * this page's address while unset; viewers see it.
 */
export function LinkAddress(props: {
  settings: NotificationSettings;
  admin: boolean;
  onSaved: (settings: NotificationSettings) => void;
}) {
  const id = createUniqueId();
  // Mounted once the settings are loaded, so it starts from them.
  const [value, setValue] = createSignal(untrack(() => props.settings.link_url ?? location.origin));
  const [error, setError] = createSignal<string | undefined>();
  const [saving, setSaving] = createSignal(false);
  const changed = () => tidy(value()) !== (props.settings.link_url ?? "");

  const put = (link_url: string | null) => {
    setSaving(true);
    api.notifiers
      .settings({ link_url })
      .then((saved) => {
        props.onSaved(saved);
        setValue(saved.link_url ?? location.origin);
        showToast({ title: saved.link_url === null ? t.cleared : t.saved, tone: "ok" });
      })
      .catch((failure: unknown) => setError(errorText(failure) ?? t.failed))
      .finally(() => setSaving(false));
  };
  const save = (event: SubmitEvent) => {
    event.preventDefault();
    const link = tidy(value());
    const problem = link === "" ? undefined : urlProblem(link);
    setError(problem);
    if (problem === undefined) put(link === "" ? null : link);
  };

  return (
    <Card aria-label={t.title}>
      <form onSubmit={save} novalidate class="flex flex-col gap-3 p-4">
        <div class="flex flex-wrap items-center gap-2">
          <h2 class="font-medium text-sm">{t.title}</h2>
          <Show when={props.settings.link_url === null}>
            <Badge tone="warn">{t.unset}</Badge>
          </Show>
        </div>
        <FormField
          label={t.label}
          for={id}
          hint={props.settings.link_url === null ? t.unsetHint : t.hint}
          error={error()}
        >
          <div class="flex flex-wrap items-center gap-2">
            <input
              id={id}
              type="text"
              inputmode="url"
              value={value()}
              readonly={!props.admin}
              onInput={(event) => {
                setValue(event.currentTarget.value);
                setError(undefined);
              }}
              autocomplete="off"
              spellcheck={false}
              aria-invalid={error() ? "true" : undefined}
              class={[
                INPUT,
                "min-w-0 flex-1 basis-64 font-mono",
                error() ? "border-crit" : "border-edge",
                { "bg-raised": !props.admin },
              ]}
            />
            <Show when={props.admin}>
              <Button
                type="submit"
                variant="primary"
                icon={Link2}
                disabled={saving() || !changed()}
              >
                {t.save}
              </Button>
              <Show when={props.settings.link_url !== null}>
                <Button variant="ghost" disabled={saving()} onClick={() => put(null)}>
                  {t.clear}
                </Button>
              </Show>
            </Show>
          </div>
        </FormField>
      </form>
    </Card>
  );
}
