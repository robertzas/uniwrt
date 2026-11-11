import { Eye, EyeOff } from "lucide";
import { createSignal, Show } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import { ApiError, api, errorText } from "../../../lib/api";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { RequestError } from "../../../ui/JobOutcome";

const t = strings.setup.rootPassword;

/**
 * The root password the vault keeps for a device (adoption.md, Identity, credentials, trust;
 * D-080): the random one adoption gave a factory-default router, or one the user had it
 * remember. Admins read it on demand, for emergencies; each view is audited.
 */
export function RootPasswordCard(props: { node: Node }) {
  const [value, setValue] = createSignal<string>();
  const [none, setNone] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [copied, setCopied] = createSignal(false);

  const show = async () => {
    setBusy(true);
    setError(undefined);
    try {
      setValue((await api.nodes.rootPassword(props.node.id)).value);
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) setNone(true);
      else setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  const copy = async () => {
    await navigator.clipboard?.writeText(value() ?? "");
    setCopied(true);
  };
  const hide = () => {
    setValue(undefined);
    setCopied(false);
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} />
      <div class="flex flex-col items-start gap-3 px-4 pb-4">
        <p class="text-fg-muted text-sm">{none() ? t.none : t.hint}</p>
        <Show
          when={value()}
          fallback={
            <Show when={!none()}>
              <Button size="sm" variant="secondary" icon={Eye} disabled={busy()} onClick={show}>
                {t.show}
              </Button>
            </Show>
          }
        >
          {(password) => (
            <>
              <output
                aria-label={t.title}
                class="select-all break-all rounded-control border border-edge bg-canvas px-3 py-2 font-mono text-sm"
              >
                {password()}
              </output>
              <div class="flex gap-2">
                <Button size="sm" variant="secondary" onClick={copy}>
                  {copied() ? t.copied : t.copy}
                </Button>
                <Button size="sm" variant="ghost" icon={EyeOff} onClick={hide}>
                  {t.hide}
                </Button>
              </div>
            </>
          )}
        </Show>
        <RequestError text={error()} />
      </div>
    </Card>
  );
}
