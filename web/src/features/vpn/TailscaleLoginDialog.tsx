import { ExternalLink } from "lucide";
import { createSignal, onSettled, Show } from "solid-js";
import type { TailscaleFacts } from "../../api/gen/TailscaleFacts";
import type { TailscaleLogin } from "../../api/gen/TailscaleLogin";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { QrCode } from "../../ui/QrCode";

const t = strings.vpn.tailscale.loginDialog;

/**
 * Logs the gateway in (network.md, VPN; D-140): starts a login when it opens (audited) and shows
 * the URL `tailscaled` hands out, as a link and a QR code, until the gateway reports it runs.
 */
export function TailscaleLoginDialog(props: {
  /** The daemon as the gateway reports it, read again often while this is open. */
  daemon: TailscaleFacts | null;
  onClose: () => void;
}) {
  const [answer, setAnswer] = createSignal<TailscaleLogin>();
  const [error, setError] = createSignal<string>();

  onSettled(() => {
    api.vpn.tailscale
      .login()
      .then(setAnswer)
      .catch((err) => setError(errorText(err)));
  });

  const done = () => (props.daemon?.state === "running" ? props.daemon : undefined);

  return (
    <Dialog open onOpenChange={(open) => !open && props.onClose()} title={t.title}>
      <div class="flex flex-col gap-4 px-5 pb-5">
        <Show
          when={answer()}
          fallback={
            <Show when={!error()}>
              <p role="status" class="py-10 text-center text-fg-muted text-sm">
                {t.starting}…
              </p>
            </Show>
          }
        >
          {(login) => (
            <Show
              when={!done() && login().auth_url}
              fallback={
                <p role="status" class="text-ok text-sm">
                  {done() ? t.done(done()?.tailnet ?? "") : t.already}
                </p>
              }
            >
              {(url) => (
                <div class="flex flex-col items-center gap-4 sm:flex-row sm:items-start">
                  <QrCode text={url()} label={t.qr} class="size-44 shrink-0" />
                  <div class="flex min-w-0 flex-col gap-2 text-sm">
                    <a
                      class="inline-flex items-center gap-1 font-medium text-accent underline-offset-2 hover:underline"
                      href={url()}
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      {t.open}
                      <Icon icon={ExternalLink} class="size-3.5" />
                    </a>
                    <p class="break-all font-mono text-fg-subtle text-xs">{url()}</p>
                    <p class="text-fg-muted text-xs">{t.hint}</p>
                    <p role="status" class="text-fg-muted text-xs">
                      {t.waiting}…
                    </p>
                  </div>
                </div>
              )}
            </Show>
          )}
        </Show>
        <RequestError text={error()} />
        <p class="text-fg-subtle text-xs">{t.audited}</p>
        <div class="flex justify-end">
          <Button onClick={() => props.onClose()}>{t.close}</Button>
        </div>
      </div>
    </Dialog>
  );
}
