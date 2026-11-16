import { createSignal, Show, untrack } from "solid-js";
import type { NodeSource } from "../../api/gen/NodeSource";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { RadioCards } from "../../ui/RadioCards";
import { TextField } from "../../ui/TextField";

const t = strings.firmware.source;

type Kind = NodeSource["kind"];
const KINDS: readonly Kind[] = ["site", "asu", "official", "github"];

/** A node's image source, as the Updates page's Source column shows it. */
export function sourceLabel(source: NodeSource, siteRepo: string | null): string {
  switch (source.kind) {
    case "site":
      return t.site(siteRepo);
    case "asu":
      return t.asu;
    case "official":
      return t.official;
    case "github":
      return t.github(source.repo);
  }
}

/**
 * Sets where a node's upgrades come from (D-122): the site's source, OpenWrt's releases (built by
 * ASU or official), or another GitHub repo, which the controller checks exists before it saves.
 */
export function SourceDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  node: number;
  name: string;
  source: NodeSource;
  siteRepo: string | null;
  onSaved: () => void;
}) {
  const [kind, setKind] = createSignal<Kind>(untrack(() => props.source.kind));
  const [repo, setRepo] = createSignal(
    untrack(() => (props.source.kind === "github" ? props.source.repo : "")),
  );
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const body = (): NodeSource =>
    kind() === "github"
      ? { kind: "github", repo: repo().trim() }
      : ({ kind: kind() } as NodeSource);
  const save = async (event: SubmitEvent) => {
    event.preventDefault();
    setSaving(true);
    setError(undefined);
    try {
      await api.firmware.source(props.node, body());
      props.onSaved();
      props.onOpenChange(false);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };
  const hint = (value: Kind) => (value === "site" ? t.hints.site(props.siteRepo) : t.hints[value]);
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={t.title(props.name)}
      description={t.description}
    >
      <form class="flex flex-col gap-4 px-5 pb-5" onSubmit={save}>
        <RadioCards
          label={t.title(props.name)}
          value={kind()}
          onChange={setKind}
          disabled={saving()}
          options={KINDS.map((value) => ({
            value,
            label: t.options[value] ?? value,
            description: hint(value),
          }))}
        />
        <Show when={kind() === "github"}>
          <TextField
            label={t.repo}
            value={repo()}
            onInput={setRepo}
            hint={t.repoHint}
            required
            disabled={saving()}
          />
        </Show>
        <RequestError text={error()} />
        <div class="flex justify-end gap-2">
          <Button size="sm" onClick={() => props.onOpenChange(false)}>
            {t.cancel}
          </Button>
          <Button
            type="submit"
            size="sm"
            variant="primary"
            disabled={saving() || (kind() === "github" && repo().trim() === "")}
          >
            {saving() ? t.saving : t.save}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
