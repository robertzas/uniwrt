import { Copy, Download } from "lucide";
import { createSignal, onSettled, Show } from "solid-js";
import type { VpnPeer } from "../../api/gen/VpnPeer";
import type { VpnPeerConfig } from "../../api/gen/VpnPeerConfig";
import { api, errorText } from "../../lib/api";
import { saveFile } from "../../lib/download";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { QrCode } from "../../ui/QrCode";

const t = strings.vpn.wireguard.config;

/**
 * A peer's configuration (network.md, VPN; D-140): the latest revision's `.conf`, read when the
 * dialog opens (audited as a secret revealed), as a QR code for the WireGuard app, as text to
 * copy and as a file to download.
 */
export function PeerConfigDialog(props: { peer: VpnPeer; onClose: () => void }) {
  const [config, setConfig] = createSignal<VpnPeerConfig>();
  const [error, setError] = createSignal<string>();
  const [copied, setCopied] = createSignal(false);

  onSettled(() => {
    api.vpn.wireguard
      .config(props.peer.id)
      .then(setConfig)
      .catch((err) => setError(errorText(err)));
  });

  const copy = async (text: string) => {
    await navigator.clipboard?.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && props.onClose()}
      title={t.title(props.peer.name)}
      size="lg"
    >
      <div class="flex flex-col gap-4 px-5 pb-5">
        <Show
          when={config()}
          fallback={
            <Show when={!error()}>
              <p role="status" class="py-10 text-center text-fg-muted text-sm">
                {t.loading}…
              </p>
            </Show>
          }
        >
          {(file) => (
            <>
              <div class="flex flex-col items-center gap-4 sm:flex-row sm:items-start">
                <QrCode text={file().conf} label={t.qr(props.peer.name)} class="size-52 shrink-0" />
                <div class="flex flex-col gap-2 text-sm">
                  <p>{t.qrHint}</p>
                  <Show when={file().own_key}>
                    <p class="text-warn">{t.ownKey}</p>
                  </Show>
                  <p class="text-fg-muted text-xs">{t.apply}</p>
                  <p class="text-fg-subtle text-xs">{t.audited}</p>
                </div>
              </div>
              <div class="flex min-w-0 flex-col gap-2">
                <div class="flex items-center gap-2">
                  <span class="font-medium text-sm">{t.file}</span>
                  <span class="truncate font-mono text-fg-subtle text-xs">{file().file_name}</span>
                  <span class="flex-1" />
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={Copy}
                    onClick={() => void copy(file().conf)}
                  >
                    {copied() ? t.copied : t.copy}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={Download}
                    onClick={() =>
                      saveFile({
                        name: file().file_name,
                        blob: new Blob([file().conf], { type: "text/plain" }),
                      })
                    }
                  >
                    {t.download}
                  </Button>
                </div>
                <pre class="max-h-72 select-all overflow-auto rounded-control border border-edge bg-canvas p-3 font-mono text-xs leading-relaxed">
                  {file().conf}
                </pre>
              </div>
            </>
          )}
        </Show>
        <RequestError text={error()} />
        <Button class="self-end" onClick={() => props.onClose()}>
          {t.close}
        </Button>
      </div>
    </Dialog>
  );
}
