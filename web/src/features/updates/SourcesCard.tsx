import { Save } from "lucide";
import { createSignal, Show, untrack } from "solid-js";
import type { SiteSources } from "../../api/gen/SiteSources";
import type { TokenChange } from "../../api/gen/TokenChange";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { JobSuccessNote, RequestError } from "../../ui/JobOutcome";
import { TextField } from "../../ui/TextField";

const t = strings.firmware.sources;

/** What happens to the kept token on save. */
type TokenMode = "keep" | "replace" | "clear";

/**
 * The site's image sources (D-122): its GitHub repo (none: OpenWrt's releases), whether
 * prereleases are offered, and an optional token sealed in the vault (shown only as kept or
 * not). Admins edit; `onChange` reads the catalog again after a save.
 */
export function SourcesCard(props: { sources: SiteSources; admin: boolean; onChange: () => void }) {
  const [repo, setRepo] = createSignal(untrack(() => props.sources.repo ?? ""));
  const [prereleases, setPrereleases] = createSignal(untrack(() => props.sources.prereleases));
  const [token, setToken] = createSignal("");
  const [mode, setMode] = createSignal<TokenMode>(
    untrack(() => (props.sources.token ? "keep" : "replace")),
  );
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [saved, setSaved] = createSignal(false);

  const tokenChange = (): TokenChange => {
    if (mode() === "clear") return { action: "clear" };
    const typed = token().trim();
    return mode() === "replace" && typed !== ""
      ? { action: "set", token: typed }
      : { action: "keep" };
  };
  const save = async (event: SubmitEvent) => {
    event.preventDefault();
    setSaving(true);
    setError(undefined);
    setSaved(false);
    try {
      const found = await api.firmware.sources({
        repo: repo().trim() === "" ? null : repo().trim(),
        prereleases: prereleases(),
        token: tokenChange(),
      });
      setRepo(found.repo ?? "");
      setToken("");
      setMode(found.token ? "keep" : "replace");
      setSaved(true);
      props.onChange();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card class="mx-auto w-full max-w-7xl" aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Show
        when={props.admin}
        fallback={
          <dl class="grid gap-x-6 gap-y-1 px-4 pb-4 text-sm sm:grid-cols-[auto_1fr]">
            <dt class="text-fg-muted">{t.repo}</dt>
            <dd class="font-mono">{props.sources.repo ?? t.openwrt}</dd>
            <dt class="text-fg-muted">{t.token}</dt>
            <dd>{props.sources.token ? t.tokenKept : t.noToken}</dd>
          </dl>
        }
      >
        <form class="flex flex-col gap-4 px-4 pb-4" onSubmit={save}>
          <div class="grid gap-4 md:grid-cols-2">
            <TextField
              label={t.repo}
              value={repo()}
              onInput={(value) => {
                setRepo(value);
                setSaved(false);
              }}
              hint={t.repoHint}
              disabled={saving()}
            />
            <Show
              when={mode() === "replace"}
              fallback={
                <div class="flex flex-col gap-1.5 text-sm">
                  <span class="font-medium text-fg">{t.token}</span>
                  <p class="text-fg-muted">{mode() === "clear" ? t.removing : t.tokenKept}</p>
                  <div class="flex gap-2">
                    <Show
                      when={mode() === "keep"}
                      fallback={
                        <Button size="sm" variant="ghost" onClick={() => setMode("keep")}>
                          {t.keep}
                        </Button>
                      }
                    >
                      <Button size="sm" onClick={() => setMode("replace")}>
                        {t.replace}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        class="text-crit"
                        onClick={() => setMode("clear")}
                      >
                        {t.remove}
                      </Button>
                    </Show>
                  </div>
                </div>
              }
            >
              <TextField
                label={t.token}
                type="password"
                autocomplete="off"
                value={token()}
                onInput={(value) => {
                  setToken(value);
                  setSaved(false);
                }}
                hint={t.tokenHint}
                disabled={saving()}
              />
            </Show>
          </div>
          <Checkbox
            label={t.prereleases}
            hint={t.prereleasesHint}
            checked={prereleases()}
            onChange={(checked) => {
              setPrereleases(checked);
              setSaved(false);
            }}
            disabled={saving()}
          />
          <RequestError text={error()} />
          <div class="flex items-center justify-end gap-3">
            <Show when={saved()}>
              <JobSuccessNote text={t.saved} />
            </Show>
            <Button type="submit" size="sm" variant="primary" icon={Save} disabled={saving()}>
              {saving() ? t.saving : t.save}
            </Button>
          </div>
        </form>
      </Show>
    </Card>
  );
}
