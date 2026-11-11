import { createMemo, createSignal, onSettled, Show } from "solid-js";
import { encode } from "uqr";
import type { Wlan } from "../../api/gen/Wlan";
import { api, errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { joinCode, needsPassphrase } from "./draft";

const t = strings.wifi.qr;

/** The modules of a QR code as one SVG path, a unit square each, inside a quiet zone. */
export function qrPath(text: string): { size: number; d: string } {
  const code = encode(text, { ecc: "M", border: 2 });
  let d = "";
  code.data.forEach((row, y) => {
    row.forEach((dark, x) => {
      if (dark) d += `M${x} ${y}h1v1h-1z`;
    });
  });
  return { size: code.size, d };
}

/**
 * A WLAN's join code for a phone's camera (wifi.md, WLANs). A passphrase is read from the vault
 * when the dialog opens, which the audit log records.
 */
export function JoinDialog(props: { wlan: Wlan; onClose: () => void }) {
  const secret = () => needsPassphrase(props.wlan.security);
  const [passphrase, setPassphrase] = createSignal<string>();
  const [error, setError] = createSignal<string>();
  const ready = () => !secret() || passphrase() !== undefined;
  const code = createMemo(() => (ready() ? qrPath(joinCode(props.wlan, passphrase())) : undefined));

  onSettled(() => {
    if (!secret()) return;
    api.site
      .reveal(`wlans/${props.wlan.id}/passphrase`)
      .then((revealed) => setPassphrase(revealed.value))
      .catch((err) => setError(errorText(err)));
  });

  return (
    <Dialog open onOpenChange={(open) => !open && props.onClose()} title={t.title(props.wlan.ssid)}>
      <div class="flex flex-col items-center gap-4 px-5 pb-5">
        <Show
          when={code()}
          fallback={
            <Show when={!error()}>
              <p role="status" class="py-10 text-fg-muted text-sm">
                {t.loading}…
              </p>
            </Show>
          }
        >
          {(qr) => (
            // Dark on white whatever the theme: cameras expect it.
            <svg
              viewBox={`0 0 ${qr().size} ${qr().size}`}
              class="size-56 rounded-control bg-white"
              role="img"
              aria-label={t.title(props.wlan.ssid)}
              shape-rendering="crispEdges"
            >
              <path d={qr().d} fill="#000" />
            </svg>
          )}
        </Show>
        <RequestError text={error()} />
        <Show when={code()}>
          <p class="text-center text-fg-muted text-sm">{t.hint}</p>
        </Show>
        <Show when={passphrase()}>
          {(value) => (
            <div class="w-full text-center">
              <p class="text-fg-subtle text-xs">{t.passphrase}</p>
              <p class="mt-0.5 select-all break-all font-mono text-sm">{value()}</p>
              <p class="mt-2 text-fg-subtle text-xs">{t.audited}</p>
            </div>
          )}
        </Show>
        <Button class="self-end" onClick={() => props.onClose()}>
          {t.close}
        </Button>
      </div>
    </Dialog>
  );
}
