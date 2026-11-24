import { Download, TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import { api, errorText } from "../../../lib/api";
import { saveFile } from "../../../lib/download";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { showToast } from "../../../ui/Toast";

const t = strings.settings.support;

/**
 * Settings → Support (observability.md, Troubleshooting assistants; D-144): what the support
 * bundle holds and, for admins, its download (`GET v1/support/bundle`, audited).
 */
export function SupportTab(props: { admin: boolean }) {
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<string>();

  const download = async () => {
    setBusy(true);
    setFailure(undefined);
    try {
      const file = await api.support.bundle();
      saveFile(file);
      showToast({ title: t.done(file.name), tone: "ok" });
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <div class="flex flex-col gap-4 px-4 pb-4 text-sm">
        <ul class="flex list-disc flex-col gap-1.5 pl-5 marker:text-fg-subtle">
          <For each={t.contents}>{(line) => <li>{line}</li>}</For>
        </ul>
        <p class="flex items-start gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5">
          <Icon icon={TriangleAlert} class="mt-0.5 size-4 shrink-0 text-warn" />
          {t.masked}
        </p>
        <Show when={props.admin} fallback={<p class="text-fg-subtle">{t.adminOnly}</p>}>
          <div class="flex flex-wrap items-center gap-3">
            <Button
              variant="primary"
              icon={Download}
              disabled={busy()}
              onClick={() => void download()}
            >
              {t.download}
            </Button>
            <Show when={busy()}>
              <span class="text-fg-muted" role="status">
                {t.collecting}
              </span>
            </Show>
          </div>
        </Show>
        <RequestError text={failure()} />
      </div>
    </Card>
  );
}
